use serde_json::Value;

#[jsonrpsee::proc_macros::rpc(client)]
trait Signal {
    #[method(name = "sendReaction", param_kind = map)]
    fn react(
        &self,
        recipient: Option<&str>,
        groupId: Option<&str>,
        emoji: &str,
        targetAuthor: &str,
        targetTimestamp: u64,
    ) -> Result<Value, ErrorObjectOwned>;

    #[method(name = "sendReceipt", param_kind = map)]
    fn receive(&self, recipient: &str, targetTimestamp: u64) -> Result<Value, ErrorObjectOwned>;

    #[method(name = "send", param_kind = map)]
    fn send(
        &self,
        recipient: Option<&str>,
        groupId: Option<&str>,
        message: &str,
        attachments: &[String],
    ) -> Result<Value, ErrorObjectOwned>;

    #[method(name = "sendTyping", param_kind = map)]
    fn send_typing(
        &self,
        recipient: Option<&str>,
        groupId: Option<&str>,
        stop: bool,
    ) -> Result<Value, ErrorObjectOwned>;

    #[subscription(name = "subscribeReceive" => "receive", unsubscribe = "unsubscribeReceive", item = Value, param_kind = map)]
    async fn subscribe_receive(&self) -> SubscriptionResult;
}
