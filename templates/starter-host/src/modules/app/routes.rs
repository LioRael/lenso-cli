use lenso::host::http::{
    ApiErrorResponse, ApiOpenApiRouter, AppContext, AppError, ErrorCode, ErrorResponse,
    HttpRequestContext, Json, JsonBody, OpenApiRouter, Path, RequestContext, State, UserActor,
    json, routes,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
struct AppStatusResponse {
    status: &'static str,
}

#[derive(Debug, Deserialize, ToSchema)]
struct CreateItemRequest {
    title: String,
}

#[derive(Debug, Deserialize, ToSchema)]
struct UpdateItemRequest {
    title: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct AppItem {
    id: i64,
    owner_user_id: String,
    title: String,
}

pub fn merge_http(base: ApiOpenApiRouter) -> ApiOpenApiRouter {
    base.merge(router())
}

fn router() -> ApiOpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(status))
        .routes(routes!(create_item))
        .routes(routes!(get_item))
        .routes(routes!(update_item))
        .routes(routes!(delete_item))
        .routes(routes!(list_items))
}

#[utoipa::path(
    get,
    path = "/v1/app/status",
    operation_id = "app_status",
    tag = "app",
    responses((
        status = 200,
        description = "App module status",
        body = AppStatusResponse,
        content_type = "application/json"
    ))
)]
async fn status() -> Json<AppStatusResponse> {
    json(AppStatusResponse { status: "ok" })
}

#[utoipa::path(
    post,
    path = "/v1/app/items",
    operation_id = "app_create_item",
    tag = "app",
    request_body(
        content = CreateItemRequest,
        content_type = "application/json",
        description = "Create an app-owned item"
    ),
    responses(
        (
            status = 200,
            description = "Item created",
            body = AppItem,
            content_type = "application/json"
        ),
        (
            status = 400,
            description = "Request validation failed",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 401,
            description = "Authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 403,
            description = "User authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 500,
            description = "Internal server error",
            body = ErrorResponse,
            content_type = "application/json"
        )
    )
)]
async fn create_item(
    State(ctx): State<AppContext>,
    actor: UserActor,
    HttpRequestContext(request_ctx): HttpRequestContext,
    JsonBody(input): JsonBody<CreateItemRequest>,
) -> Result<Json<AppItem>, ApiErrorResponse> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(ApiErrorResponse::with_context(
            AppError::new(ErrorCode::Validation, "item title is required"),
            &request_ctx,
        ));
    }

    let row = sqlx::query(
        r#"
        insert into app.items (owner_user_id, title)
        values ($1, $2)
        returning id, owner_user_id, title
        "#,
    )
    .bind(&actor.user_id)
    .bind(title)
    .fetch_one(&ctx.db)
    .await
    .map_err(|error| database_error(error, &request_ctx))?;

    Ok(json(item_from_row(row, &request_ctx)?))
}

#[utoipa::path(
    get,
    path = "/v1/app/items",
    operation_id = "app_list_items",
    tag = "app",
    responses(
        (
            status = 200,
            description = "Recent app-owned items for the authenticated user",
            body = Vec<AppItem>,
            content_type = "application/json"
        ),
        (
            status = 401,
            description = "Authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 403,
            description = "User authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 500,
            description = "Internal server error",
            body = ErrorResponse,
            content_type = "application/json"
        )
    )
)]
async fn list_items(
    State(ctx): State<AppContext>,
    actor: UserActor,
    HttpRequestContext(request_ctx): HttpRequestContext,
) -> Result<Json<Vec<AppItem>>, ApiErrorResponse> {
    let rows = sqlx::query(
        r#"
        select id, owner_user_id, title
        from app.items
        where owner_user_id = $1
        order by id desc
        limit 50
        "#,
    )
    .bind(&actor.user_id)
    .fetch_all(&ctx.db)
    .await
    .map_err(|error| database_error(error, &request_ctx))?;
    let items = rows
        .into_iter()
        .map(|row| item_from_row(row, &request_ctx))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(json(items))
}

#[utoipa::path(
    get,
    path = "/v1/app/items/{id}",
    operation_id = "app_get_item",
    tag = "app",
    params(("id" = i64, Path, description = "App item id")),
    responses(
        (
            status = 200,
            description = "App-owned item",
            body = AppItem,
            content_type = "application/json"
        ),
        (
            status = 404,
            description = "Item not found",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 401,
            description = "Authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 403,
            description = "User authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 500,
            description = "Internal server error",
            body = ErrorResponse,
            content_type = "application/json"
        )
    )
)]
async fn get_item(
    State(ctx): State<AppContext>,
    actor: UserActor,
    HttpRequestContext(request_ctx): HttpRequestContext,
    Path(id): Path<i64>,
) -> Result<Json<AppItem>, ApiErrorResponse> {
    let row = sqlx::query(
        r#"
        select id, owner_user_id, title
        from app.items
        where id = $1 and owner_user_id = $2
        "#,
    )
    .bind(id)
    .bind(&actor.user_id)
    .fetch_optional(&ctx.db)
    .await
    .map_err(|error| database_error(error, &request_ctx))?
    .ok_or_else(|| {
        ApiErrorResponse::with_context(
            AppError::new(ErrorCode::NotFound, format!("app item {id} was not found")),
            &request_ctx,
        )
    })?;

    Ok(json(item_from_row(row, &request_ctx)?))
}

#[utoipa::path(
    patch,
    path = "/v1/app/items/{id}",
    operation_id = "app_update_item",
    tag = "app",
    params(("id" = i64, Path, description = "App item id")),
    request_body(
        content = UpdateItemRequest,
        content_type = "application/json",
        description = "Update an app-owned item"
    ),
    responses(
        (
            status = 200,
            description = "Item updated",
            body = AppItem,
            content_type = "application/json"
        ),
        (
            status = 400,
            description = "Request validation failed",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 404,
            description = "Item not found",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 401,
            description = "Authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 403,
            description = "User authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 500,
            description = "Internal server error",
            body = ErrorResponse,
            content_type = "application/json"
        )
    )
)]
async fn update_item(
    State(ctx): State<AppContext>,
    actor: UserActor,
    HttpRequestContext(request_ctx): HttpRequestContext,
    Path(id): Path<i64>,
    JsonBody(input): JsonBody<UpdateItemRequest>,
) -> Result<Json<AppItem>, ApiErrorResponse> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(ApiErrorResponse::with_context(
            AppError::new(ErrorCode::Validation, "item title is required"),
            &request_ctx,
        ));
    }

    let row = sqlx::query(
        r#"
        update app.items
        set title = $1
        where id = $2 and owner_user_id = $3
        returning id, owner_user_id, title
        "#,
    )
    .bind(title)
    .bind(id)
    .bind(&actor.user_id)
    .fetch_optional(&ctx.db)
    .await
    .map_err(|error| database_error(error, &request_ctx))?
    .ok_or_else(|| {
        ApiErrorResponse::with_context(
            AppError::new(ErrorCode::NotFound, format!("app item {id} was not found")),
            &request_ctx,
        )
    })?;

    Ok(json(item_from_row(row, &request_ctx)?))
}

#[utoipa::path(
    delete,
    path = "/v1/app/items/{id}",
    operation_id = "app_delete_item",
    tag = "app",
    params(("id" = i64, Path, description = "App item id")),
    responses(
        (
            status = 200,
            description = "Item deleted",
            body = AppItem,
            content_type = "application/json"
        ),
        (
            status = 404,
            description = "Item not found",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 401,
            description = "Authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 403,
            description = "User authentication is required",
            body = ErrorResponse,
            content_type = "application/json"
        ),
        (
            status = 500,
            description = "Internal server error",
            body = ErrorResponse,
            content_type = "application/json"
        )
    )
)]
async fn delete_item(
    State(ctx): State<AppContext>,
    actor: UserActor,
    HttpRequestContext(request_ctx): HttpRequestContext,
    Path(id): Path<i64>,
) -> Result<Json<AppItem>, ApiErrorResponse> {
    let row = sqlx::query(
        r#"
        delete from app.items
        where id = $1 and owner_user_id = $2
        returning id, owner_user_id, title
        "#,
    )
    .bind(id)
    .bind(&actor.user_id)
    .fetch_optional(&ctx.db)
    .await
    .map_err(|error| database_error(error, &request_ctx))?
    .ok_or_else(|| {
        ApiErrorResponse::with_context(
            AppError::new(ErrorCode::NotFound, format!("app item {id} was not found")),
            &request_ctx,
        )
    })?;

    Ok(json(item_from_row(row, &request_ctx)?))
}

fn item_from_row(
    row: sqlx::postgres::PgRow,
    request_ctx: &RequestContext,
) -> Result<AppItem, ApiErrorResponse> {
    Ok(AppItem {
        id: row
            .try_get("id")
            .map_err(|error| database_error(error, request_ctx))?,
        owner_user_id: row
            .try_get("owner_user_id")
            .map_err(|error| database_error(error, request_ctx))?,
        title: row
            .try_get("title")
            .map_err(|error| database_error(error, request_ctx))?,
    })
}

fn database_error(error: sqlx::Error, request_ctx: &RequestContext) -> ApiErrorResponse {
    ApiErrorResponse::with_context(
        AppError::new(ErrorCode::Internal, "App item database operation failed").with_source(error),
        request_ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_documents_app_routes() {
        let document = router().to_openapi();

        assert!(document.paths.paths.contains_key("/v1/app/status"));
        let items = document
            .paths
            .paths
            .get("/v1/app/items")
            .expect("items path should be documented");
        assert!(items.get.is_some());
        assert!(items.post.is_some());
        let item = document
            .paths
            .paths
            .get("/v1/app/items/{id}")
            .expect("item detail path should be documented");
        assert!(item.get.is_some());
        assert!(item.patch.is_some());
        assert!(item.delete.is_some());
    }
}
