use serde_json::json;

use crate::client::{
    mock::{run_sink_tasks, SinkTask, TestServerBuilder},
    Client,
};
use crate::config::{Config, MergeStrategy, RpcDefinitions, RpcSubscription, ServerConfig};
use crate::server::start_server;

#[tokio::test]
async fn merge_subscription_works() {
    let subscribe_head = "chain_subscribeNewHeads";
    let update_head = "chain_newHead";
    let unsubscribe_head = "chain_unsubscribeNewHeads";

    let subscribe_finalized = "chain_subscribeFinalizedHeads";
    let update_finalized = "chain_finalizedHead";
    let unsubscribe_finalized = "chain_unsubscribeFinalizedHeads";

    let subscribe_mock = "mock_sub";
    let unsubscribe_mock = "mock_unsub";
    let update_mock = "mock";

    let mut builder = TestServerBuilder::new();

    let mut head_sub = builder.register_subscription(subscribe_head, update_head, unsubscribe_head);
    let mut finalized_sub =
        builder.register_subscription(subscribe_finalized, update_finalized, unsubscribe_finalized);
    let mut mock_sub_rx =
        builder.register_subscription(subscribe_mock, update_mock, unsubscribe_mock);

    let (addr, _upstream_handle) = builder.build().await;

    tokio::spawn(async move {
        let (_, head_sub_rx) = head_sub.recv().await.unwrap();
        let (_, finalized_sub_rx) = finalized_sub.recv().await.unwrap();

        run_sink_tasks(&head_sub_rx, vec![SinkTask::Send(json!(1))]).await;
        run_sink_tasks(&finalized_sub_rx, vec![SinkTask::Send(json!(1))]).await;
    });

    let config = Config {
        endpoints: vec![format!("ws://{addr}")],
        stale_timeout_seconds: 0,
        cache_ttl_seconds: None,
        merge_subscription_keep_alive_seconds: Some(1),
        server: ServerConfig {
            listen_address: "0.0.0.0".to_string(),
            port: 0,
            max_connections: 10,
        },
        rpcs: RpcDefinitions {
            methods: vec![],
            subscriptions: vec![
                RpcSubscription {
                    subscribe: subscribe_head.to_string(),
                    unsubscribe: unsubscribe_head.to_string(),
                    name: update_head.to_string(),
                    merge_strategy: None,
                },
                RpcSubscription {
                    subscribe: subscribe_finalized.to_string(),
                    unsubscribe: unsubscribe_finalized.to_string(),
                    name: update_finalized.to_string(),
                    merge_strategy: None,
                },
                RpcSubscription {
                    subscribe: subscribe_mock.to_string(),
                    unsubscribe: unsubscribe_mock.to_string(),
                    name: update_mock.to_string(),
                    merge_strategy: Some(MergeStrategy::MergeStorageChanges),
                },
            ],
            aliases: vec![],
        },
        telemetry: None,
        health: None,
    };

    let (addr, server) = start_server(
        &config,
        Client::new(&[format!("ws://{addr}")]).await.unwrap(),
    )
    .await
    .unwrap();

    let client = Client::new(&[format!("ws://{addr}")]).await.unwrap();
    let mut first_sub = client
        .subscribe(subscribe_mock, vec![], unsubscribe_mock)
        .await
        .unwrap();

    let send_msg = tokio::spawn(async move {
        let (_, mock_sub_sink) = mock_sub_rx.recv().await.unwrap();

        run_sink_tasks(
            &mock_sub_sink,
            vec![
                SinkTask::Send(json!({
                    "block": "0x01",
                    "changes": [
                        ["0x01", "hello"],
                        ["0x02", null]
                    ]
                })),
                SinkTask::Sleep(100),
                SinkTask::Send(json!({
                    "block": "0x02",
                    "changes": [
                        ["0x02", "world"]
                    ]
                })),
                SinkTask::Sleep(100),
                SinkTask::Send(json!({
                    "block": "0x03",
                    "changes": [
                        ["0x01", null],
                        ["0x02", "bye"]
                    ]
                })),
                SinkTask::Sleep(100),
                SinkTask::Send(json!({
                    "block": "0x04",
                    "changes": [
                        ["0x01", "hello"],
                        ["0x02", "again"]
                    ]
                })),
                // after 1s upstream subscription is dropped
                SinkTask::SinkClosed(Some(1)),
            ],
        )
        .await;
    });

    let test_one = tokio::spawn(async move {
        assert_eq!(
            first_sub.next().await.unwrap().unwrap(),
            json!({
                "block": "0x01",
                "changes": [
                    ["0x01", "hello"],
                    ["0x02", null]
                ]
            })
        );

        assert_eq!(
            first_sub.next().await.unwrap().unwrap(),
            json!({
                "block": "0x02",
                "changes": [
                    ["0x02", "world"],
                ]
            })
        );

        assert_eq!(
            first_sub.next().await.unwrap().unwrap(),
            json!({
                "block": "0x03",
                "changes": [
                    ["0x01", null],
                    ["0x02", "bye"]
                ]
            })
        );

        // first subscription will unsubscribe but it shouldn't affect second subscription
        first_sub.unsubscribe().await.unwrap();
    });

    // second subscription happens after 2nd msg is send (100ms) and 3rd msg (200ms)
    // so 1st msg for the second subscription will be a merge between 1st & 2nd msg ["block": "0x02"]
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let mut second_sub = client
        .subscribe(subscribe_mock, vec![], unsubscribe_mock)
        .await
        .unwrap();

    let test_two = tokio::spawn(async move {
        // 2nd msg with merged storage changes
        assert_eq!(
            second_sub.next().await.unwrap().unwrap(),
            json!({
                "block": "0x02",
                "changes": [
                    ["0x01", "hello"],
                    ["0x02", "world"],

                ]
            })
        );

        // 3rd msg is the same as the first subscription is getting
        assert_eq!(
            second_sub.next().await.unwrap().unwrap(),
            json!({
                "block": "0x03",
                "changes": [
                    ["0x01", null],
                    ["0x02", "bye"]
                ]
            })
        );

        // got 4th msg
        assert_eq!(
            second_sub.next().await.unwrap().unwrap(),
            json!({
                "block": "0x04",
                "changes": [
                    ["0x01", "hello"],
                    ["0x02", "again"]
                ]
            })
        );

        second_sub.unsubscribe().await.unwrap();
    });

    send_msg.await.unwrap();
    test_one.await.unwrap();
    test_two.await.unwrap();

    // stop server
    server.stop().unwrap();
}
