use kube::CustomResourceExt;
fn main() {
    print!(
        "{}",
        serde_yaml::to_string(&n8n_rustful_operator::Single::crd()).unwrap()
    );
    println!("---");
    print!(
        "{}",
        serde_yaml::to_string(&n8n_rustful_operator::Cluster::crd()).unwrap()
    );
}
