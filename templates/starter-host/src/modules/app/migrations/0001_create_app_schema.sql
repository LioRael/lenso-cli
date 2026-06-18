create schema if not exists app;

create table if not exists app.items (
    id bigserial primary key,
    owner_user_id text not null,
    title text not null check (length(trim(title)) > 0),
    created_at timestamptz not null default now()
);

create index if not exists items_owner_user_id_idx on app.items (owner_user_id);
