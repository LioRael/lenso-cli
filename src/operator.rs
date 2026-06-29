use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::json;

#[derive(Clone, Debug)]
pub struct ExportCrdBundleOptions {
    pub output: PathBuf,
    pub image: String,
    pub namespace: String,
    pub json: bool,
}

impl From<&crate::OperatorExportCrdArgs> for ExportCrdBundleOptions {
    fn from(args: &crate::OperatorExportCrdArgs) -> Self {
        Self {
            output: args.output.clone(),
            image: args.image.clone(),
            namespace: args.namespace.clone(),
            json: args.json,
        }
    }
}

pub fn export_crd_bundle(options: ExportCrdBundleOptions) -> Result<()> {
    fs::create_dir_all(&options.output)
        .with_context(|| format!("create directory {}", options.output.display()))?;
    let files = [
        ("lenso.dev_lensoserviceproviders.yaml", crd_yaml()),
        ("rbac.yaml", rbac_yaml(&options.namespace)),
        (
            "deployment.yaml",
            deployment_yaml(&options.namespace, &options.image),
        ),
        ("kustomization.yaml", kustomization_yaml()),
        ("README.md", readme(&options.namespace)),
    ];
    for (name, contents) in files {
        fs::write(options.output.join(name), contents)
            .with_context(|| format!("write {}", options.output.join(name).display()))?;
    }

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "output": options.output,
                "namespace": options.namespace,
                "files": [
                    "lenso.dev_lensoserviceproviders.yaml",
                    "rbac.yaml",
                    "deployment.yaml",
                    "kustomization.yaml",
                    "README.md"
                ]
            }))?
        );
    } else {
        println!("Wrote Lenso Operator bundle: {}", options.output.display());
        println!("next: kubectl apply -k {}", options.output.display());
    }
    Ok(())
}

fn crd_yaml() -> String {
    r#"apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: lensoserviceproviders.lenso.dev
spec:
  group: lenso.dev
  names:
    kind: LensoServiceProvider
    plural: lensoserviceproviders
    singular: lensoserviceprovider
  scope: Namespaced
  versions:
    - name: v1alpha1
      served: true
      storage: true
      subresources:
        status: {}
      additionalPrinterColumns:
        - name: State
          type: string
          jsonPath: .status.state
        - name: Release
          type: string
          jsonPath: .status.observedReleaseId
        - name: Image
          type: string
          jsonPath: .status.observedImage
        - name: Ready
          type: integer
          jsonPath: .status.readyReplicas
      schema:
        openAPIV3Schema:
          type: object
          required: [spec]
          properties:
            spec:
              type: object
              required: [serviceName, environment, image, port]
              properties:
                serviceName: { type: string }
                environment: { type: string }
                image: { type: string }
                releaseId: { type: string }
                manifestReference: { type: string }
                modules:
                  type: array
                  items: { type: string }
                replicas:
                  type: integer
                  format: int32
                  default: 1
                port:
                  type: integer
                  format: int32
                envFrom:
                  type: object
                  properties:
                    configMap: { type: string }
                    secret: { type: string }
                ingress:
                  type: object
                  properties:
                    host: { type: string }
                autoscaling:
                  type: object
                  properties:
                    enabled: { type: boolean }
                    minReplicas: { type: integer, format: int32, default: 1 }
                    maxReplicas: { type: integer, format: int32, default: 3 }
                    targetCpuUtilization: { type: integer, format: int32, default: 70 }
                disruptionBudget:
                  type: object
                  properties:
                    enabled: { type: boolean }
                    minAvailable: { type: integer, format: int32, default: 1 }
                networkPolicy:
                  type: object
                  properties:
                    enabled: { type: boolean }
            status:
              type: object
              properties:
                state: { type: string }
                observedGeneration: { type: integer, format: int64 }
                observedReleaseId: { type: string }
                observedImage: { type: string }
                readyReplicas: { type: integer, format: int32 }
                desiredReplicas: { type: integer, format: int32 }
                availableReplicas: { type: integer, format: int32 }
                manifestReference: { type: string }
                conditions:
                  type: array
                  items:
                    type: object
                    properties:
                      type: { type: string }
                      status: { type: string }
                      reason: { type: string }
                      message: { type: string }
                      lastTransitionTime: { type: string }
"#
    .to_owned()
}

fn rbac_yaml(namespace: &str) -> String {
    format!(
        r#"apiVersion: v1
kind: Namespace
metadata:
  name: {namespace}
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: lenso-operator
  namespace: {namespace}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: lenso-operator
rules:
  - apiGroups: ["lenso.dev"]
    resources: ["lensoserviceproviders", "lensoserviceproviders/status"]
    verbs: ["get", "list", "watch", "patch", "update"]
  - apiGroups: ["apps"]
    resources: ["deployments"]
    verbs: ["get", "list", "watch", "patch"]
  - apiGroups: [""]
    resources: ["services"]
    verbs: ["get", "list", "watch", "patch"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["ingresses", "networkpolicies"]
    verbs: ["get", "list", "watch", "patch"]
  - apiGroups: ["autoscaling"]
    resources: ["horizontalpodautoscalers"]
    verbs: ["get", "list", "watch", "patch"]
  - apiGroups: ["policy"]
    resources: ["poddisruptionbudgets"]
    verbs: ["get", "list", "watch", "patch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: lenso-operator
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: lenso-operator
subjects:
  - kind: ServiceAccount
    name: lenso-operator
    namespace: {namespace}
"#
    )
}

fn deployment_yaml(namespace: &str, image: &str) -> String {
    format!(
        r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: lenso-operator
  namespace: {namespace}
  labels:
    app.kubernetes.io/name: lenso-operator
    app.kubernetes.io/part-of: lenso
spec:
  replicas: 1
  selector:
    matchLabels:
      app.kubernetes.io/name: lenso-operator
  template:
    metadata:
      labels:
        app.kubernetes.io/name: lenso-operator
        app.kubernetes.io/part-of: lenso
    spec:
      serviceAccountName: lenso-operator
      containers:
        - name: lenso-operator
          image: {image}
          env:
            - name: RUST_LOG
              value: lenso_operator=info,kube=info
"#
    )
}

fn kustomization_yaml() -> String {
    "resources:\n  - lenso.dev_lensoserviceproviders.yaml\n  - rbac.yaml\n  - deployment.yaml\n"
        .to_owned()
}

fn readme(namespace: &str) -> String {
    format!(
        "# Lenso Operator\n\n```sh\nkubectl apply -k .\nkubectl get deployment lenso-operator -n {namespace}\n```\n"
    )
}
