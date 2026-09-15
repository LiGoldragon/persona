//! Closed ordinary Persona Signal vocabulary for item 13's draft.

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SubscriptionId(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineDatom(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirstPrompt(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservedAt(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceTimestamp(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResetInterval(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemainingAmount(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaSample {
    pub subscription: SubscriptionId,
    pub source_timestamp: SourceTimestamp,
    pub reset_interval: ResetInterval,
    pub remaining: RemainingAmount,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Query {
    RecordQuota(QuotaSample),
    AssembleFirstPrompt(FirstPrompt),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnknownQuota {
    FutureSourceTimestamp,
    ZeroRemainingInterval,
    SupersededWindow,
    StorageUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Response {
    QuotaRecorded(QuotaSample),
    QuotaUnknown(UnknownQuota),
    FirstPromptAssembled(FirstPrompt),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Signal {
    Query(Query),
    Response(Response),
}
