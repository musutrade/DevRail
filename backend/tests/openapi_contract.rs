use arc_admin_backend::{openapi, API_PREFIX, API_ROUTE_CONTRACT};
use axum::http::Uri;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn generated_openapi() -> serde_json::Value {
    serde_json::to_value(openapi::document()).expect("serialize generated OpenAPI")
}

#[test]
fn openapi_operations_match_backend_contract() {
    let document = generated_openapi();
    let servers = document["servers"].as_array().expect("OpenAPI servers");
    assert!(!servers.is_empty(), "OpenAPI has at least one server");
    for server in servers {
        let url = server["url"].as_str().expect("OpenAPI server URL");
        let uri = server["url"]
            .as_str()
            .expect("server URL")
            .parse::<Uri>()
            .unwrap_or_else(|error| panic!("parse OpenAPI server URL {url:?}: {error}"));
        assert_eq!(uri.path(), API_PREFIX, "OpenAPI server path drift");
    }

    let http_methods = ["get", "post", "put", "patch", "delete"];
    let mut documented = BTreeSet::new();
    for (path, operations) in document["paths"].as_object().expect("OpenAPI paths") {
        for method in http_methods {
            if operations.get(method).is_some() {
                documented.insert((format!("{API_PREFIX}{path}"), method.to_string()));
            }
        }
    }

    let expected = API_ROUTE_CONTRACT
        .iter()
        .flat_map(|(path, methods)| {
            methods
                .iter()
                .map(|method| ((*path).to_string(), (*method).to_string()))
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(documented, expected, "OpenAPI route contract drift");
}

#[test]
fn checked_in_openapi_artifact_matches_rust_types() {
    let specification_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/openapi.json");
    let checked_in = std::fs::read_to_string(&specification_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", specification_path.display()));
    let generated = format!(
        "{}\n",
        serde_json::to_string_pretty(&openapi::document()).expect("serialize generated OpenAPI")
    );
    assert_eq!(
        checked_in, generated,
        "运行 cargo run --bin export_openapi 更新契约"
    );
}
