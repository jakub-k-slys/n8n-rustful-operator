use kube::CustomResourceExt;
fn main() {
    print!(
        "{}",
        serde_yaml::to_string(&n8n_rustful_operator::N8nInstance::crd()).unwrap()
    )
}
