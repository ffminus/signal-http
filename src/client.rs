use serde_json::Value;

#[jsonrpsee::proc_macros::rpc(client)]
trait Signal {
    #[subscription(name = "subscribeReceive" => "receive", unsubscribe = "unsubscribeReceive", item = Value, param_kind = map)]
    async fn subscribe_receive(&self) -> SubscriptionResult;
}
