use serde_json::Value;

#[jsonrpsee::proc_macros::rpc(client)]
trait Signal {
    #[method(name = "send", param_kind = map)]
    fn send(
        &self,
        recipient: Option<&str>,
        groupId: Option<&str>,
        message: &str,
        attachments: &[String],
    ) -> Result<Value, ErrorObjectOwned>;

    #[subscription(name = "subscribeReceive" => "receive", unsubscribe = "unsubscribeReceive", item = Value, param_kind = map)]
    async fn subscribe_receive(&self) -> SubscriptionResult;
}
