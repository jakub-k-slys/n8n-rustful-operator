//! Publishes the mTLS material produced by `bootstrap-mtls.sh` as two Secrets.
//!
//! Runs as the second container of the `<p>-sandbox-certs` Job. Only the files
//! on the whitelists below are copied — the root CA private key stays in the
//! Job's emptyDir and dies with it.

use k8s_openapi::api::core::v1::Secret;
use kube::{
    Client,
    api::{Api, ObjectMeta, PostParams},
};
use std::{collections::BTreeMap, path::Path};

pub const API_FILES: &[&str] = &[
    "ca.crt",
    "grpc-server.crt",
    "grpc-server.key",
    "control-grpc-api-client.crt",
    "control-grpc-api-client.key",
];

pub const RUNNER_FILES: &[&str] = &[
    "ca.crt",
    "grpc-client.crt",
    "grpc-client.key",
    "control-grpc-server.crt",
    "control-grpc-server.key",
];

#[derive(Clone, Copy)]
pub enum Side {
    Api,
    Runner,
}

impl Side {
    fn dir(self) -> &'static str {
        match self {
            Side::Api => "api",
            Side::Runner => "runner",
        }
    }
    fn files(self) -> &'static [&'static str] {
        match self {
            Side::Api => API_FILES,
            Side::Runner => RUNNER_FILES,
        }
    }
}

/// Read the whitelisted files for one side into Secret `stringData`.
/// Everything not on the whitelist is ignored, by design.
pub fn collect(dir: &Path, side: Side) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for f in side.files() {
        let path = dir.join(side.dir()).join(f);
        let body = std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        out.insert((*f).to_string(), body);
    }
    Ok(out)
}

pub fn parse_labels(pairs: &[String]) -> Result<BTreeMap<String, String>, String> {
    pairs
        .iter()
        .map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("label {p:?} is not k=v"))
        })
        .collect()
}

fn flag(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].clone())
        .ok_or_else(|| format!("missing required flag {name}"))
}

fn repeated(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].clone())
        .collect()
}

async fn publish(
    api: &Api<Secret>,
    name: &str,
    ns: &str,
    labels: &BTreeMap<String, String>,
    data: BTreeMap<String, String>,
) -> Result<(), String> {
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        string_data: Some(data),
        type_: Some("Opaque".to_string()),
        ..Default::default()
    };
    match api.create(&PostParams::default(), &secret).await {
        Ok(_) => Ok(()),
        // Another run got there first: the desired state already holds.
        Err(kube::Error::Api(ae)) if ae.code == 409 => Ok(()),
        Err(e) => Err(format!("creating Secret {name}: {e}")),
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("tlspub: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let dir = flag(&args, "--dir")?;
    let ns = flag(&args, "--namespace")?;
    let api_secret = flag(&args, "--api-secret")?;
    let runner_secret = flag(&args, "--runner-secret")?;
    let labels = parse_labels(&repeated(&args, "--label"))?;

    let dir = Path::new(&dir);
    let api_data = collect(dir, Side::Api)?;
    let runner_data = collect(dir, Side::Runner)?;

    let client = Client::try_default()
        .await
        .map_err(|e| format!("kube client: {e}"))?;
    let secrets: Api<Secret> = Api::namespaced(client, &ns);
    publish(&secrets, &api_secret, &ns, &labels, api_data).await?;
    publish(&secrets, &runner_secret, &ns, &labels, runner_data).await?;
    println!("tlspub: published {api_secret} and {runner_secret} in {ns}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tlspub-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("api")).unwrap();
        fs::create_dir_all(d.join("runner")).unwrap();
        d
    }

    fn write(dir: &std::path::Path, rel: &str, body: &str) {
        fs::write(dir.join(rel), body).unwrap();
    }

    #[test]
    fn collects_exactly_the_five_api_files() {
        let d = tmpdir("api");
        for f in API_FILES {
            write(&d, &format!("api/{f}"), f);
        }
        let out = collect(&d, Side::Api).unwrap();
        let mut keys: Vec<&String> = out.keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "ca.crt",
                "control-grpc-api-client.crt",
                "control-grpc-api-client.key",
                "grpc-server.crt",
                "grpc-server.key"
            ]
        );
        assert_eq!(out["ca.crt"], "ca.crt");
    }

    #[test]
    fn never_publishes_the_ca_private_key() {
        let d = tmpdir("cakey");
        for f in API_FILES {
            write(&d, &format!("api/{f}"), f);
        }
        write(&d, "api/ca.key", "SUPER SECRET CA KEY");
        write(&d, "api/root-ca.key", "SUPER SECRET CA KEY");
        let out = collect(&d, Side::Api).unwrap();
        assert!(!out.contains_key("ca.key"));
        assert!(!out.contains_key("root-ca.key"));
        assert!(!out.values().any(|v| v.contains("SUPER SECRET")));
    }

    #[test]
    fn collects_exactly_the_five_runner_files() {
        let d = tmpdir("runner");
        for f in RUNNER_FILES {
            write(&d, &format!("runner/{f}"), f);
        }
        let out = collect(&d, Side::Runner).unwrap();
        assert_eq!(out.len(), 5);
        assert!(out.contains_key("control-grpc-server.key"));
    }

    #[test]
    fn fails_loudly_when_a_whitelisted_file_is_missing() {
        let d = tmpdir("missing");
        write(&d, "api/ca.crt", "x");
        let err = collect(&d, Side::Api).unwrap_err();
        assert!(err.contains("grpc-server.crt"));
    }

    #[test]
    fn parses_repeated_label_flags() {
        let labels = parse_labels(&["a=1".to_string(), "b=2".to_string()]).unwrap();
        assert_eq!(labels["a"], "1");
        assert_eq!(labels["b"], "2");
    }

    #[test]
    fn rejects_a_malformed_label() {
        assert!(parse_labels(&["nope".to_string()]).is_err());
    }
}
