use pushkind_emailer::domain::types::EmailRecipientReply;

/// Updates to apply to an email recipient record.
pub struct UpdateEmailRecipient<'a> {
    pub sent: Option<bool>,
    pub opened: Option<bool>,
    pub reply: Option<&'a EmailRecipientReply>,
}
