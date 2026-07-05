pub trait PayloadString {
    fn as_str(&self) -> &str;
}

macro_rules! impl_payload_string {
    ($($type:ty),+ $(,)?) => {
        $(
            impl PayloadString for $type {
                fn as_str(&self) -> &str {
                    self.payload().as_str()
                }
            }
        )+
    };
}

impl_payload_string!(
    signal_persona::ComponentName,
    signal_persona::EngineIdentifier,
    signal_persona::StateDirectoryPath,
    signal_persona::DomainSocketPath,
    signal_persona::EngineManagementSocketPath,
    signal_persona::ManagerSocketPath,
    signal_persona::SystemPrincipal,
    meta_signal_persona::EngineLabel,
);

pub trait UnixUserIdentifierValue {
    fn as_u32(&self) -> u32;
}

impl UnixUserIdentifierValue for signal_persona::UnixUserIdentifier {
    fn as_u32(&self) -> u32 {
        *self.payload() as u32
    }
}

pub trait EngineGenerationValue {
    fn into_u64(self) -> u64;
}

impl EngineGenerationValue for meta_signal_persona::EngineGeneration {
    fn into_u64(self) -> u64 {
        self.into_payload()
    }
}
