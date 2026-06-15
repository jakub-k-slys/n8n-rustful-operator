use crate::{
    reconciler::{cluster, single},
    spec::{Cluster, Single},
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
    let ctx = state.to_context(client).await;
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
    futures::future::join(single_ctrl, cluster_ctrl).await;
}
