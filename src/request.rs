use std::ffi::OsString;
use std::path::PathBuf;

use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode, NotaRecord};

use crate::error::{Error, Result};
use crate::schema::{PersonaDocument, PersonaObject};

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct DescribeSchema {}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ValidateObject {
    pub object: PersonaObject,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ValidateDocument {
    pub document: PersonaDocument,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaRequest {
    DescribeSchema(DescribeSchema),
    ValidateObject(ValidateObject),
    ValidateDocument(ValidateDocument),
}

impl PersonaRequest {
    pub fn from_nota(text: &str) -> Result<Self> {
        let mut decoder = Decoder::nota(text);
        let request = Self::decode(&mut decoder)?;
        if let Some(token) = decoder.peek_token()? {
            return Err(nota_codec::Error::UnexpectedToken {
                expected: "end of input",
                got: token,
            }
            .into());
        }
        Ok(request)
    }

    pub fn into_output(self) -> PersonaOutput {
        match self {
            Self::DescribeSchema(_) => PersonaOutput::SchemaExample(SchemaExample {
                document: PersonaDocument::example(),
            }),
            Self::ValidateObject(request) => PersonaOutput::ValidatedObject(ValidatedObject {
                object: request.object,
            }),
            Self::ValidateDocument(request) => {
                PersonaOutput::ValidatedDocument(ValidatedDocument {
                    document: request.document,
                })
            }
        }
    }
}

impl NotaEncode for PersonaRequest {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        match self {
            Self::DescribeSchema(request) => request.encode(encoder),
            Self::ValidateObject(request) => request.encode(encoder),
            Self::ValidateDocument(request) => request.encode(encoder),
        }
    }
}

impl NotaDecode for PersonaRequest {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        let head = decoder.peek_record_head()?;
        match head.as_str() {
            "DescribeSchema" => Ok(Self::DescribeSchema(DescribeSchema::decode(decoder)?)),
            "ValidateObject" => Ok(Self::ValidateObject(ValidateObject::decode(decoder)?)),
            "ValidateDocument" => Ok(Self::ValidateDocument(ValidateDocument::decode(decoder)?)),
            other => Err(nota_codec::Error::UnknownKindForVerb {
                verb: "PersonaRequest",
                got: other.to_string(),
            }),
        }
    }
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct SchemaExample {
    pub document: PersonaDocument,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ValidatedObject {
    pub object: PersonaObject,
}

#[derive(NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ValidatedDocument {
    pub document: PersonaDocument,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaOutput {
    SchemaExample(SchemaExample),
    ValidatedObject(ValidatedObject),
    ValidatedDocument(ValidatedDocument),
}

impl kameo::reply::Reply for PersonaOutput {
    type Ok = Self;
    type Error = kameo::error::Infallible;
    type Value = Self;

    fn to_result(self) -> std::result::Result<Self::Ok, Self::Error> {
        Ok(self)
    }

    fn into_any_err(self) -> Option<Box<dyn kameo::reply::ReplyError>> {
        None
    }

    fn into_value(self) -> Self::Value {
        self
    }
}

impl PersonaOutput {
    pub fn to_nota(&self) -> Result<String> {
        let mut encoder = Encoder::nota();
        self.encode(&mut encoder)?;
        Ok(encoder.into_string())
    }
}

impl NotaEncode for PersonaOutput {
    fn encode(&self, encoder: &mut Encoder) -> nota_codec::Result<()> {
        match self {
            Self::SchemaExample(output) => output.encode(encoder),
            Self::ValidatedObject(output) => output.encode(encoder),
            Self::ValidatedDocument(output) => output.encode(encoder),
        }
    }
}

impl NotaDecode for PersonaOutput {
    fn decode(decoder: &mut Decoder<'_>) -> nota_codec::Result<Self> {
        let head = decoder.peek_record_head()?;
        match head.as_str() {
            "SchemaExample" => Ok(Self::SchemaExample(SchemaExample::decode(decoder)?)),
            "ValidatedObject" => Ok(Self::ValidatedObject(ValidatedObject::decode(decoder)?)),
            "ValidatedDocument" => Ok(Self::ValidatedDocument(ValidatedDocument::decode(decoder)?)),
            other => Err(nota_codec::Error::UnknownKindForVerb {
                verb: "PersonaOutput",
                got: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    arguments: Vec<OsString>,
}

impl CommandLine {
    pub fn from_env() -> Self {
        Self::from_arguments(std::env::args_os().skip(1))
    }

    pub fn from_arguments<I, S>(arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            arguments: arguments.into_iter().map(Into::into).collect(),
        }
    }

    pub fn decode_request(&self) -> Result<PersonaRequest> {
        match self.arguments.first() {
            Some(first) if CommandLineArgument::new(first).starts_inline_record() => {
                PersonaRequest::from_nota(&self.inline_nota_text()?)
            }
            Some(first) => {
                self.require_single_path_argument()?;
                RequestFile::from_path(PathBuf::from(first)).decode()
            }
            None => Ok(PersonaRequest::DescribeSchema(DescribeSchema {})),
        }
    }

    fn inline_nota_text(&self) -> Result<String> {
        let mut parts = Vec::new();
        for argument in &self.arguments {
            let Some(text) = argument.to_str() else {
                return Err(Error::InvalidInlineNotaArgument {
                    got: format!("{argument:?}"),
                });
            };
            parts.push(text.to_string());
        }
        Ok(parts.join(" "))
    }

    fn require_single_path_argument(&self) -> Result<()> {
        if let Some(argument) = self.arguments.get(1) {
            return Err(Error::UnexpectedArgument {
                got: argument.to_string_lossy().to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFile {
    path: PathBuf,
}

impl RequestFile {
    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn decode(&self) -> Result<PersonaRequest> {
        let text = std::fs::read_to_string(&self.path)?;
        PersonaRequest::from_nota(&text)
    }
}

struct CommandLineArgument<'argument> {
    argument: &'argument OsString,
}

impl<'argument> CommandLineArgument<'argument> {
    fn new(argument: &'argument OsString) -> Self {
        Self { argument }
    }

    fn starts_inline_record(&self) -> bool {
        self.argument.to_string_lossy().starts_with('(')
    }
}
