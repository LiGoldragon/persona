//! Closed privileged Persona Signal vocabulary for item 13's draft.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriptionId(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirstPrompt(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Query {
    AssembleMetaFirstPrompt(FirstPrompt),
    InspectSubscription(SubscriptionId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Response {
    MetaFirstPromptAssembled(FirstPrompt),
    SubscriptionInspected(SubscriptionId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetaSignal {
    Query(Query),
    Response(Response),
}
