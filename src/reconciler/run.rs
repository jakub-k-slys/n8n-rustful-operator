use crate::{
    reconciler::{assistant, cluster, single},
    spec::{Assistant, Cluster, Single},
    state::State,
};
use futures::StreamExt;
use kube::{
    api::{Api, ListParams},
    client::Client,
    runtime::controller::Controller,
};
use tracing::*;

pub async fn run(state: State) {
    let client = Client::try_default().await.expect("failed to create kube Client");
    let singles = Api::<Single>::all(client.clone());
    if let Err(e) = singles.list(&ListParams::default().limit(1)).await {
        error!("Single CRD is not queryable; {e:?}. Is it installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }
    let clusters = Api::<Cluster>::all(client.clone());
    if let Err(e) = clusters.list(&ListParams::default().limit(1)).await {
        error!("Cluster CRD is not queryable; {e:?}. Is it installed?");
        std::process::exit(1);
    }
    let assistants = Api::<Assistant>::all(client.clone());
    if let Err(e) = assistants.list(&ListParams::default().limit(1)).await {
        error!("Assistant CRD is not queryable; {e:?}. Is it installed?");
        std::process::exit(1);
    }
    let ctx = state.to_context(client).await;
    let ctx2 = ctx.clone();
    let single_ctrl = Controller::new(singles, single::watcher_config())
        .shutdown_on_signal()
        .run(single::reconcile, single::error_policy, ctx.clone())
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()));
    let cluster_ctrl = Controller::new(clusters, cluster::watcher_config())
        .shutdown_on_signal()
        .run(cluster::reconcile, cluster::error_policy, ctx)
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()));
    let assistant_ctrl = Controller::new(assistants, assistant::watcher_config())
        .shutdown_on_signal()
        .run(assistant::reconcile, assistant::error_policy, ctx2)
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()));
    futures::future::join3(single_ctrl, cluster_ctrl, assistant_ctrl).await;
}
