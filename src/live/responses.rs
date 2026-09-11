//! Typed shared Responses items and nested delegated event helpers.
//!
//! The input declarations follow all 33 alternatives of the public Live
//! `response.item.create.item` schema, including their embedded shared tool
//! descriptions. These are conversation items, **not** additional permissions to
//! register tools: Live session registration only supports function and web search.
//! Optional nullable fields use [`Field`]; required nullable fields use [`Nullable`].
//! Unknown outbound fields are rejected rather than silently discarded. All
//! content-bearing `Debug` implementations redact their fields.
//!
//! Schema source: <https://developers.openai.com/api/reference/resources/live>.
//! Delegation behavior: <https://developers.openai.com/api/docs/guides/live-delegation>.

use super::models::{Field, Nullable, nonnull};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Default)]
struct Bounds {
    min_len: Option<usize>,
    max_len: Option<usize>,
    min: Option<i64>,
    max: Option<i64>,
    min_number: Option<f64>,
    max_number: Option<f64>,
    min_items: Option<usize>,
    max_items: Option<usize>,
    max_properties: Option<usize>,
    key_max_len: Option<usize>,
    identifier: bool,
    tunnel_id: bool,
    uri: bool,
}

trait Check {
    fn check(&self, bounds: &Bounds) -> Result<(), String>;
}

impl Check for String {
    fn check(&self, bounds: &Bounds) -> Result<(), String> {
        let length = self.chars().count();
        if bounds.min_len.is_some_and(|min| length < min)
            || bounds.max_len.is_some_and(|max| length > max)
        {
            return Err("string length outside the documented bounds".into());
        }
        if bounds.uri && url::Url::parse(self).is_err() {
            return Err("expected an absolute URI".into());
        }
        if bounds.identifier
            && (self.is_empty()
                || !self
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
        {
            return Err("expected an ASCII identifier".into());
        }
        if bounds.tunnel_id
            && !self.strip_prefix("tunnel_").is_some_and(|suffix| {
                suffix.len() == 32
                    && suffix
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            })
        {
            return Err("expected a documented tunnel identifier".into());
        }
        Ok(())
    }
}

impl Check for f64 {
    fn check(&self, bounds: &Bounds) -> Result<(), String> {
        if !self.is_finite()
            || bounds.min_number.is_some_and(|min| *self < min)
            || bounds.max_number.is_some_and(|max| *self > max)
        {
            return Err("number outside the documented bounds".into());
        }
        Ok(())
    }
}

impl Check for i64 {
    fn check(&self, bounds: &Bounds) -> Result<(), String> {
        if bounds.min.is_some_and(|min| *self < min) || bounds.max.is_some_and(|max| *self > max) {
            return Err("integer outside the documented bounds".into());
        }
        Ok(())
    }
}

impl Check for bool {
    fn check(&self, _: &Bounds) -> Result<(), String> {
        Ok(())
    }
}

impl Check for Value {
    fn check(&self, _: &Bounds) -> Result<(), String> {
        Ok(())
    }
}

impl<T: Check> Check for Option<T> {
    fn check(&self, bounds: &Bounds) -> Result<(), String> {
        self.as_ref().map_or(Ok(()), |value| value.check(bounds))
    }
}

impl<T: Check> Check for Field<T> {
    fn check(&self, bounds: &Bounds) -> Result<(), String> {
        self.value().map_or(Ok(()), |value| value.check(bounds))
    }
}

impl<T: Check> Check for Nullable<T> {
    fn check(&self, bounds: &Bounds) -> Result<(), String> {
        self.0.check(bounds)
    }
}

impl<T: Check> Check for Vec<T> {
    fn check(&self, bounds: &Bounds) -> Result<(), String> {
        if bounds.min_items.is_some_and(|min| self.len() < min)
            || bounds.max_items.is_some_and(|max| self.len() > max)
        {
            return Err("array length outside the documented bounds".into());
        }
        self.iter().try_for_each(|value| value.check(bounds))
    }
}

impl<T: Check> Check for BTreeMap<String, T> {
    fn check(&self, bounds: &Bounds) -> Result<(), String> {
        if bounds.max_properties.is_some_and(|max| self.len() > max)
            || bounds
                .key_max_len
                .is_some_and(|max| self.keys().any(|key| key.chars().count() > max))
        {
            return Err("record size outside the documented bounds".into());
        }
        self.values().try_for_each(|value| value.check(bounds))
    }
}

fn required<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    T::deserialize(deserializer)
}

macro_rules! redacted_debug {
    ($name:ident) => {
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), " { ..redacted.. }"))
            }
        }
    };
}

// The four explicit groups keep wire requiredness separate from Rust defaults.
// In particular required nullable/Unknown values must never default on omission.
macro_rules! model {
    ($(#[$meta:meta])* $name:ident {
        required { $($r:ident: $rt:ty $([$($rb:ident: $rv:expr),*])?),* }
        optional { $($o:ident: $ot:ty $([$($ob:ident: $ov:expr),*])?),* }
        nullable { $($n:ident: $nt:ty $([$($nb:ident: $nv:expr),*])?),* }
        required_nullable { $($q:ident: $qt:ty $([$($qb:ident: $qv:expr),*])?),* }
    }) => {
        $(#[$meta])*
        #[doc = ""]
        #[doc = concat!("Typed shared Responses schema: `", stringify!($name), "`.")]
        // A uniform schema macro also covers records containing floating-point values.
        #[allow(clippy::derive_partial_eq_without_eq)]
        #[derive(Clone, PartialEq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            $(
                #[doc = concat!("Required wire field `", stringify!($r), "`.")]
                #[serde(deserialize_with = "required")]
                pub $r: $rt,
            )*
            $(
                #[doc = concat!("Optional, non-null wire field `", stringify!($o), "`.")]
                #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "nonnull")]
                pub $o: Option<$ot>,
            )*
            $(
                #[doc = concat!("Optional nullable wire field `", stringify!($n), "`.")]
                #[serde(default, skip_serializing_if = "Field::is_absent")]
                pub $n: Field<$nt>,
            )*
            $(
                #[doc = concat!("Required nullable wire field `", stringify!($q), "`.")]
                #[serde(deserialize_with = "required")]
                pub $q: Nullable<$qt>,
            )*
        }

        redacted_debug!($name);

        impl Check for $name {
            fn check(&self, _: &Bounds) -> Result<(), String> {
                $(
                    self.$r.check(&Bounds { $($($rb: $rv,)*)? ..Bounds::default() })
                        .map_err(|e| format!("{}.{}: {e}", stringify!($name), stringify!($r)))?;
                )*
                $(
                    self.$o.check(&Bounds { $($($ob: $ov,)*)? ..Bounds::default() })
                        .map_err(|e| format!("{}.{}: {e}", stringify!($name), stringify!($o)))?;
                )*
                $(
                    self.$n.check(&Bounds { $($($nb: $nv,)*)? ..Bounds::default() })
                        .map_err(|e| format!("{}.{}: {e}", stringify!($name), stringify!($n)))?;
                )*
                $(
                    self.$q.check(&Bounds { $($($qb: $qv,)*)? ..Bounds::default() })
                        .map_err(|e| format!("{}.{}: {e}", stringify!($name), stringify!($q)))?;
                )*
                Ok(())
            }
        }
    };
}

macro_rules! literals {
    ($name:ident { $first:ident = $first_wire:literal $(, $variant:ident = $wire:literal)* }) => {
        #[doc = concat!("Documented wire literals for `", stringify!($name), "`.")]
        #[allow(clippy::enum_variant_names)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name {
            #[doc = concat!("`", $first_wire, "`")]
            #[default]
            #[serde(rename = $first_wire)]
            $first,
            $(
                #[doc = concat!("`", $wire, "`")]
                #[serde(rename = $wire)]
                $variant,
            )*
        }

        impl Check for $name {
            fn check(&self, _: &Bounds) -> Result<(), String> {
                Ok(())
            }
        }
    };
}

macro_rules! union {
    ($(#[$meta:meta])* $name:ident { $($variant:ident($ty:ty)),+ }) => {
        $(#[$meta])*
        #[doc = ""]
        #[doc = concat!("All documented alternatives for `", stringify!($name), "`.")]
        // Alternatives retain protocol names and may contain floating-point values.
        #[allow(clippy::derive_partial_eq_without_eq, clippy::enum_variant_names)]
        #[derive(Clone, PartialEq, Serialize, Deserialize)]
        #[serde(untagged)]
        pub enum $name {
            $(
                #[doc = concat!("The `", stringify!($ty), "` alternative.")]
                $variant($ty),
            )+
        }

        redacted_debug!($name);

        impl Check for $name {
            fn check(&self, bounds: &Bounds) -> Result<(), String> {
                match self {
                    $(Self::$variant(value) => value.check(bounds),)+
                }
            }
        }
    };
}

literals! { ResponseInputTextType { InputText = "input_text" } }
literals! { ResponseInputTextPromptCacheBreakpointMode { Explicit = "explicit" } }
model! { ResponseInputTextPromptCacheBreakpoint {
    required { mode: ResponseInputTextPromptCacheBreakpointMode }
    optional { }
    nullable { }
    required_nullable { }
} }
model! { ResponseInputText {
    required { text: String, r#type: ResponseInputTextType }
    optional { prompt_cache_breakpoint: ResponseInputTextPromptCacheBreakpoint }
    nullable { }
    required_nullable { }
} }
literals! { ResponseImageDetail { Low = "low", High = "high", Auto = "auto", Original = "original" } }
literals! { ResponseInputImageType { InputImage = "input_image" } }
model! { ResponseInputImage {
    required { detail: ResponseImageDetail, r#type: ResponseInputImageType }
    optional { prompt_cache_breakpoint: ResponseInputTextPromptCacheBreakpoint }
    nullable { file_id: String, image_url: String [uri: true] }
    required_nullable { }
} }
literals! { ResponseInputFileType { InputFile = "input_file" } }
literals! { ResponseInputFileDetail { Auto = "auto", Low = "low", High = "high" } }
model! { ResponseInputFile {
    required { r#type: ResponseInputFileType }
    optional { detail: ResponseInputFileDetail, file_data: String, file_url: String [uri: true], filename: String, prompt_cache_breakpoint: ResponseInputTextPromptCacheBreakpoint }
    nullable { file_id: String }
    required_nullable { }
} }
union! { ResponseInputContent { InputText(ResponseInputText), InputImage(ResponseInputImage), InputFile(ResponseInputFile) } }
union! { ResponseEasyInputMessageContent { TextInput(String), ListResponseInputContent(Vec<ResponseInputContent>) } }
literals! { ResponseEasyInputMessageRole { User = "user", Assistant = "assistant", System = "system", Developer = "developer" } }
literals! { ResponseEasyInputMessagePhase { Commentary = "commentary", FinalAnswer = "final_answer" } }
literals! { ResponseEasyInputMessageType { Message = "message" } }
model! { ResponseEasyInputMessage {
    required { content: ResponseEasyInputMessageContent, role: ResponseEasyInputMessageRole }
    optional { r#type: ResponseEasyInputMessageType }
    nullable { phase: ResponseEasyInputMessagePhase }
    required_nullable { }
} }
literals! { ResponseMessageRole { User = "user", System = "system", Developer = "developer" } }
literals! { ResponseMessageStatus { InProgress = "in_progress", Completed = "completed", Incomplete = "incomplete" } }
model! { ResponseMessage {
    required { content: Vec<ResponseInputContent>, role: ResponseMessageRole }
    optional { status: ResponseMessageStatus, r#type: ResponseEasyInputMessageType }
    nullable { }
    required_nullable { }
} }
literals! { ResponseFileCitationType { FileCitation = "file_citation" } }
model! { ResponseFileCitation {
    required { file_id: String, filename: String, index: i64, r#type: ResponseFileCitationType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseURLCitationType { UrlCitation = "url_citation" } }
model! { ResponseURLCitation {
    required { end_index: i64, start_index: i64, title: String, r#type: ResponseURLCitationType, url: String [uri: true] }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseContainerFileCitationType { ContainerFileCitation = "container_file_citation" } }
model! { ResponseContainerFileCitation {
    required { container_id: String, end_index: i64, file_id: String, filename: String, start_index: i64, r#type: ResponseContainerFileCitationType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseFilePathType { FilePath = "file_path" } }
model! { ResponseFilePath {
    required { file_id: String, index: i64, r#type: ResponseFilePathType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseOutputTextAnnotationsEntry { FileCitation(ResponseFileCitation), URLCitation(ResponseURLCitation), ContainerFileCitation(ResponseContainerFileCitation), FilePath(ResponseFilePath) } }
model! { ResponseOutputTextLogprobsEntryTopLogprobsEntry {
    required { token: String, bytes: Vec<i64>, logprob: f64 }
    optional { }
    nullable { }
    required_nullable { }
} }
model! { ResponseOutputTextLogprobsEntry {
    required { token: String, bytes: Vec<i64>, logprob: f64, top_logprobs: Vec<ResponseOutputTextLogprobsEntryTopLogprobsEntry> }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseOutputTextType { OutputText = "output_text" } }
model! { ResponseOutputText {
    required { annotations: Vec<ResponseOutputTextAnnotationsEntry>, logprobs: Vec<ResponseOutputTextLogprobsEntry>, text: String, r#type: ResponseOutputTextType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseOutputRefusalType { Refusal = "refusal" } }
model! { ResponseOutputRefusal {
    required { refusal: String, r#type: ResponseOutputRefusalType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseOutputMessageContentEntry { OutputText(ResponseOutputText), OutputRefusal(ResponseOutputRefusal) } }
literals! { ResponseOutputMessageRole { Assistant = "assistant" } }
model! { ResponseOutputMessage {
    required { id: String, content: Vec<ResponseOutputMessageContentEntry>, role: ResponseOutputMessageRole, status: ResponseMessageStatus, r#type: ResponseEasyInputMessageType }
    optional { }
    nullable { phase: ResponseEasyInputMessagePhase }
    required_nullable { }
} }
literals! { ResponseFileSearchCallStatus { InProgress = "in_progress", Searching = "searching", Completed = "completed", Incomplete = "incomplete", Failed = "failed" } }
literals! { ResponseFileSearchCallType { FileSearchCall = "file_search_call" } }
union! { ResponseFileSearchCallResultsEntryAttributesValue { String(String), F64(f64), Bool(bool) } }
model! { ResponseFileSearchCallResultsEntry {
    required { }
    optional { file_id: String, filename: String, score: f64 [min_number: Some(0.0), max_number: Some(1.0)], text: String }
    nullable { attributes: BTreeMap<String, ResponseFileSearchCallResultsEntryAttributesValue> [max_properties: Some(16), key_max_len: Some(64), max_len: Some(512)] }
    required_nullable { }
} }
model! { ResponseFileSearchCall {
    required { id: String, queries: Vec<String>, status: ResponseFileSearchCallStatus, r#type: ResponseFileSearchCallType }
    optional { }
    nullable { results: Vec<ResponseFileSearchCallResultsEntry> }
    required_nullable { }
} }
model! { ResponseComputerCallPendingSafetyChecksEntry {
    required { id: String }
    optional { }
    nullable { code: String, message: String }
    required_nullable { }
} }
literals! { ResponseComputerCallType { ComputerCall = "computer_call" } }
literals! { ResponseClickButton { Left = "left", Right = "right", Wheel = "wheel", Back = "back", Forward = "forward" } }
literals! { ResponseClickType { Click = "click" } }
model! { ResponseClick {
    required { button: ResponseClickButton, r#type: ResponseClickType, x: i64, y: i64 }
    optional { }
    nullable { keys: Vec<String> }
    required_nullable { }
} }
literals! { ResponseDoubleClickType { DoubleClick = "double_click" } }
model! { ResponseDoubleClick {
    required { r#type: ResponseDoubleClickType, x: i64, y: i64 }
    optional { }
    nullable { }
    required_nullable { keys: Vec<String> }
} }
model! { ResponseDragPathEntry {
    required { x: i64, y: i64 }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseDragType { Drag = "drag" } }
model! { ResponseDrag {
    required { path: Vec<ResponseDragPathEntry>, r#type: ResponseDragType }
    optional { }
    nullable { keys: Vec<String> }
    required_nullable { }
} }
literals! { ResponseKeypressType { Keypress = "keypress" } }
model! { ResponseKeypress {
    required { keys: Vec<String>, r#type: ResponseKeypressType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseMoveType { Move = "move" } }
model! { ResponseMove {
    required { r#type: ResponseMoveType, x: i64, y: i64 }
    optional { }
    nullable { keys: Vec<String> }
    required_nullable { }
} }
literals! { ResponseScreenshotType { Screenshot = "screenshot" } }
model! { ResponseScreenshot {
    required { r#type: ResponseScreenshotType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseScrollType { Scroll = "scroll" } }
model! { ResponseScroll {
    required { scroll_x: i64, scroll_y: i64, r#type: ResponseScrollType, x: i64, y: i64 }
    optional { }
    nullable { keys: Vec<String> }
    required_nullable { }
} }
literals! { ResponseTypeType { Type = "type" } }
model! { ResponseType {
    required { text: String, r#type: ResponseTypeType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseWaitType { Wait = "wait" } }
model! { ResponseWait {
    required { r#type: ResponseWaitType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseComputerAction { Click(ResponseClick), DoubleClick(ResponseDoubleClick), Drag(ResponseDrag), Keypress(ResponseKeypress), Move(ResponseMove), Screenshot(ResponseScreenshot), Scroll(ResponseScroll), Type(ResponseType), Wait(ResponseWait) } }
model! { ResponseComputerCall {
    required { id: String, call_id: String, pending_safety_checks: Vec<ResponseComputerCallPendingSafetyChecksEntry>, status: ResponseMessageStatus, r#type: ResponseComputerCallType }
    optional { action: ResponseComputerAction, actions: Vec<ResponseComputerAction> }
    nullable { }
    required_nullable { }
} }
literals! { ResponseComputerToolCallOutputScreenshotType { ComputerScreenshot = "computer_screenshot" } }
model! { ResponseComputerToolCallOutputScreenshot {
    required { r#type: ResponseComputerToolCallOutputScreenshotType }
    optional { file_id: String, image_url: String [uri: true] }
    nullable { }
    required_nullable { }
} }
literals! { ResponseComputerCallOutputType { ComputerCallOutput = "computer_call_output" } }
model! { ResponseComputerCallOutput {
    required { call_id: String [min_len: Some(1), max_len: Some(64)], output: ResponseComputerToolCallOutputScreenshot, r#type: ResponseComputerCallOutputType }
    optional { }
    nullable { id: String, acknowledged_safety_checks: Vec<ResponseComputerCallPendingSafetyChecksEntry>, status: ResponseMessageStatus }
    required_nullable { }
} }
literals! { ResponseSearchType { Search = "search" } }
literals! { ResponseSearchSourcesEntryType { Url = "url" } }
model! { ResponseSearchSourcesEntry {
    required { r#type: ResponseSearchSourcesEntryType, url: String [uri: true] }
    optional { }
    nullable { }
    required_nullable { }
} }
model! { ResponseSearch {
    required { r#type: ResponseSearchType }
    optional { queries: Vec<String>, query: String, sources: Vec<ResponseSearchSourcesEntry> }
    nullable { }
    required_nullable { }
} }
literals! { ResponseOpenPageType { OpenPage = "open_page" } }
model! { ResponseOpenPage {
    required { r#type: ResponseOpenPageType }
    optional { }
    nullable { url: String [uri: true] }
    required_nullable { }
} }
literals! { ResponseFindInPageType { FindInPage = "find_in_page" } }
model! { ResponseFindInPage {
    required { pattern: String, r#type: ResponseFindInPageType, url: String [uri: true] }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseWebSearchCallAction { Search(ResponseSearch), OpenPage(ResponseOpenPage), FindInPage(ResponseFindInPage) } }
literals! { ResponseWebSearchCallStatus { InProgress = "in_progress", Searching = "searching", Completed = "completed", Failed = "failed", Incomplete = "incomplete" } }
literals! { ResponseWebSearchCallType { WebSearchCall = "web_search_call" } }
model! { ResponseWebSearchCall {
    required { id: String, action: ResponseWebSearchCallAction, status: ResponseWebSearchCallStatus, r#type: ResponseWebSearchCallType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseFunctionCallType { FunctionCall = "function_call" } }
literals! { ResponseDirectType { Direct = "direct" } }
model! { ResponseDirect {
    required { r#type: ResponseDirectType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseProgramType { Program = "program" } }
model! { ResponseProgram {
    required { caller_id: String, r#type: ResponseProgramType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseFunctionCallCaller { Direct(ResponseDirect), Program(ResponseProgram) } }
model! {
    /// A function-call item supplied as history, not an actionable inbound event.
    /// Use [`ResponseEvent::completed_function_call`] to select finished calls.
    ResponseFunctionCall {
    required { arguments: String, call_id: String, name: String, r#type: ResponseFunctionCallType }
    optional { id: String, r#async: bool, namespace: String, status: ResponseMessageStatus }
    nullable { caller: ResponseFunctionCallCaller }
    required_nullable { }
} }
model! { ResponseInputTextContent {
    required { text: String [max_len: Some(10_485_760)], r#type: ResponseInputTextType }
    optional { }
    nullable { prompt_cache_breakpoint: ResponseInputTextPromptCacheBreakpoint }
    required_nullable { }
} }
model! { ResponseInputImageContent {
    required { r#type: ResponseInputImageType }
    optional { }
    nullable { detail: ResponseImageDetail, file_id: String, image_url: String [max_len: Some(20_971_520), uri: true], prompt_cache_breakpoint: ResponseInputTextPromptCacheBreakpoint }
    required_nullable { }
} }
model! { ResponseInputFileContent {
    required { r#type: ResponseInputFileType }
    optional { detail: ResponseInputFileDetail }
    nullable { file_data: String [max_len: Some(73_400_320)], file_id: String, file_url: String [uri: true], filename: String, prompt_cache_breakpoint: ResponseInputTextPromptCacheBreakpoint }
    required_nullable { }
} }
union! {
    /// A text, image, or file content part returned by an application function.
    /// These tool-result parts have different nullability and length constraints
    /// from the corresponding message-input parts.
    ResponseToolOutputContent { InputTextContent(ResponseInputTextContent), InputImageContent(ResponseInputImageContent), InputFileContent(ResponseInputFileContent) }
}
/// A function result encoded as text or an ordered list of content parts.
///
/// To return structured JSON, serialize it into the text alternative. The text
/// alternative is limited to 10,485,760 characters; content parts have their own
/// independent bounds.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseFunctionOutput {
    Text(String),
    Content(Vec<ResponseToolOutputContent>),
}

redacted_debug!(ResponseFunctionOutput);

impl Check for ResponseFunctionOutput {
    fn check(&self, _: &Bounds) -> Result<(), String> {
        match self {
            Self::Text(text) => text.check(&Bounds {
                max_len: Some(10_485_760),
                ..Bounds::default()
            }),
            Self::Content(parts) => parts.check(&Bounds::default()),
        }
    }
}
literals! { ResponseFunctionCallOutputType { FunctionCallOutput = "function_call_output" } }
model! { ResponseProgramCaller {
    required { caller_id: String [min_len: Some(1), max_len: Some(64)], r#type: ResponseProgramType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseToolCaller { Direct(ResponseDirect), Program(ResponseProgramCaller) } }
model! { ResponseFunctionCallOutput {
    required { output: ResponseFunctionOutput, r#type: ResponseFunctionCallOutputType }
    optional { }
    nullable { id: String, call_id: String [min_len: Some(1), max_len: Some(64)], caller: ResponseToolCaller, name: String [min_len: Some(1), max_len: Some(128)], namespace: String [min_len: Some(1), max_len: Some(64), identifier: true], status: ResponseMessageStatus }
    required_nullable { }
} }
literals! { ResponseToolSearchCallType { ToolSearchCall = "tool_search_call" } }
literals! { ResponseToolSearchCallExecution { Server = "server", Client = "client" } }
model! { ResponseToolSearchCall {
    required { arguments: BTreeMap<String, Value>, r#type: ResponseToolSearchCallType }
    optional { execution: ResponseToolSearchCallExecution }
    nullable { id: String, call_id: String [min_len: Some(1), max_len: Some(64)], status: ResponseMessageStatus }
    required_nullable { }
} }
literals! { ResponseFunctionType { Function = "function" } }
literals! { ResponseFunctionAllowedCallersEntry { Direct = "direct", Programmatic = "programmatic" } }
model! { ResponseFunction {
    required { name: String, r#type: ResponseFunctionType }
    optional { r#async: bool, defer_loading: bool }
    nullable { allowed_callers: Vec<ResponseFunctionAllowedCallersEntry>, description: String, output_schema: BTreeMap<String, Value> }
    required_nullable { parameters: BTreeMap<String, Value>, strict: bool }
} }
literals! { ResponseFileSearchType { FileSearch = "file_search" } }
literals! { ResponseComparisonFilterType { Eq = "eq", Ne = "ne", Gt = "gt", Gte = "gte", Lt = "lt", Lte = "lte", In = "in", Nin = "nin" } }
union! { ResponseFilterScalar { String(String), F64(f64) } }
union! { ResponseComparisonFilterValue { String(String), F64(f64), Bool(bool), List(Vec<ResponseFilterScalar>) } }
model! { ResponseComparisonFilter {
    required { key: String, r#type: ResponseComparisonFilterType, value: ResponseComparisonFilterValue }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseCompoundFilterFiltersEntry { ComparisonFilter(ResponseComparisonFilter), CompoundFilter(ResponseCompoundFilter) } }
literals! { ResponseCompoundFilterType { And = "and", Or = "or" } }
model! { ResponseCompoundFilter {
    required { filters: Vec<ResponseCompoundFilterFiltersEntry>, r#type: ResponseCompoundFilterType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseFileSearchFilters { ComparisonFilter(ResponseComparisonFilter), CompoundFilter(ResponseCompoundFilter) } }
model! { ResponseFileSearchRankingOptionsHybridSearch {
    required { embedding_weight: f64, text_weight: f64 }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseFileSearchRankingOptionsRanker { Auto = "auto", Default20241115 = "default-2024-11-15" } }
model! { ResponseFileSearchRankingOptions {
    required { }
    optional { hybrid_search: ResponseFileSearchRankingOptionsHybridSearch, ranker: ResponseFileSearchRankingOptionsRanker, score_threshold: f64 [min_number: Some(0.0), max_number: Some(1.0)] }
    nullable { }
    required_nullable { }
} }
model! { ResponseFileSearch {
    required { r#type: ResponseFileSearchType, vector_store_ids: Vec<String> }
    optional { max_num_results: i64 [min: Some(1), max: Some(50)], ranking_options: ResponseFileSearchRankingOptions }
    nullable { filters: ResponseFileSearchFilters }
    required_nullable { }
} }
literals! { ResponseComputerType { Computer = "computer" } }
model! { ResponseComputer {
    required { r#type: ResponseComputerType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseComputerUsePreviewEnvironment { Windows = "windows", Mac = "mac", Linux = "linux", Ubuntu = "ubuntu", Browser = "browser" } }
literals! { ResponseComputerUsePreviewType { ComputerUsePreview = "computer_use_preview" } }
model! { ResponseComputerUsePreview {
    required { display_height: i64, display_width: i64, environment: ResponseComputerUsePreviewEnvironment, r#type: ResponseComputerUsePreviewType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseWebSearchType { WebSearch = "web_search", WebSearch20250826 = "web_search_2025_08_26" } }
model! { ResponseWebSearchFilters {
    required { }
    optional { }
    nullable { allowed_domains: Vec<String> }
    required_nullable { }
} }
literals! { ResponseWebSearchSearchContextSize { Low = "low", Medium = "medium", High = "high" } }
literals! { ResponseWebSearchUserLocationType { Approximate = "approximate" } }
model! { ResponseWebSearchUserLocation {
    required { }
    optional { r#type: ResponseWebSearchUserLocationType }
    nullable { city: String, country: String, region: String, timezone: String }
    required_nullable { }
} }
model! { ResponseWebSearch {
    required { r#type: ResponseWebSearchType }
    optional { external_web_access: bool, search_context_size: ResponseWebSearchSearchContextSize }
    nullable { filters: ResponseWebSearchFilters, user_location: ResponseWebSearchUserLocation }
    required_nullable { }
} }
literals! { ResponseMcpType { Mcp = "mcp" } }
model! { ResponseMcpToolFilter {
    required { }
    optional { read_only: bool, tool_names: Vec<String> }
    nullable { }
    required_nullable { }
} }
union! { ResponseMcpAllowedTools { McpAllowedTools(Vec<String>), McpToolFilter(ResponseMcpToolFilter) } }
literals! { ResponseMcpConnectorId { ConnectorDropbox = "connector_dropbox", ConnectorGmail = "connector_gmail", ConnectorGooglecalendar = "connector_googlecalendar", ConnectorGoogledrive = "connector_googledrive", ConnectorMicrosoftteams = "connector_microsoftteams", ConnectorOutlookcalendar = "connector_outlookcalendar", ConnectorOutlookemail = "connector_outlookemail", ConnectorSharepoint = "connector_sharepoint" } }
model! { ResponseMcpToolApprovalFilter {
    required { }
    optional { always: ResponseMcpToolFilter, never: ResponseMcpToolFilter }
    nullable { }
    required_nullable { }
} }
literals! { ResponseMcpToolApprovalSetting { Always = "always", Never = "never" } }
union! { ResponseMcpRequireApproval { McpToolApprovalFilter(ResponseMcpToolApprovalFilter), McpToolApprovalSetting(ResponseMcpToolApprovalSetting) } }
model! { ResponseMcp {
    required { server_label: String, r#type: ResponseMcpType }
    optional { authorization: String, connector_id: ResponseMcpConnectorId, defer_loading: bool, server_description: String, server_url: String [uri: true], tunnel_id: String [tunnel_id: true] }
    nullable { allowed_callers: Vec<ResponseFunctionAllowedCallersEntry> [min_items: Some(1)], allowed_tools: ResponseMcpAllowedTools, headers: BTreeMap<String, String>, require_approval: ResponseMcpRequireApproval }
    required_nullable { }
} }
literals! { ResponseCodeInterpreterToolAutoType { Auto = "auto" } }
literals! { ResponseCodeInterpreterToolAutoMemoryLimit { N1g = "1g", N4g = "4g", N16g = "16g", N64g = "64g" } }
literals! { ResponseContainerNetworkPolicyDisabledType { Disabled = "disabled" } }
model! { ResponseContainerNetworkPolicyDisabled {
    required { r#type: ResponseContainerNetworkPolicyDisabledType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseContainerNetworkPolicyAllowlistType { Allowlist = "allowlist" } }
model! { ResponseContainerNetworkPolicyDomainSecret {
    required { domain: String [min_len: Some(1)], name: String [min_len: Some(1)], value: String [min_len: Some(1), max_len: Some(10_485_760)] }
    optional { }
    nullable { }
    required_nullable { }
} }
model! { ResponseContainerNetworkPolicyAllowlist {
    required { allowed_domains: Vec<String> [min_items: Some(1)], r#type: ResponseContainerNetworkPolicyAllowlistType }
    optional { domain_secrets: Vec<ResponseContainerNetworkPolicyDomainSecret> [min_items: Some(1)] }
    nullable { }
    required_nullable { }
} }
union! { ResponseCodeInterpreterToolAutoNetworkPolicy { ContainerNetworkPolicyDisabled(ResponseContainerNetworkPolicyDisabled), ContainerNetworkPolicyAllowlist(ResponseContainerNetworkPolicyAllowlist) } }
model! { ResponseCodeInterpreterToolAuto {
    required { r#type: ResponseCodeInterpreterToolAutoType }
    optional { file_ids: Vec<String> [max_items: Some(50)], network_policy: ResponseCodeInterpreterToolAutoNetworkPolicy }
    nullable { memory_limit: ResponseCodeInterpreterToolAutoMemoryLimit }
    required_nullable { }
} }
union! { ResponseCodeInterpreterContainer { String(String), CodeInterpreterToolAuto(ResponseCodeInterpreterToolAuto) } }
literals! { ResponseCodeInterpreterType { CodeInterpreter = "code_interpreter" } }
model! { ResponseCodeInterpreter {
    required { container: ResponseCodeInterpreterContainer, r#type: ResponseCodeInterpreterType }
    optional { }
    nullable { allowed_callers: Vec<ResponseFunctionAllowedCallersEntry> [min_items: Some(1)] }
    required_nullable { }
} }
literals! { ResponseProgrammaticToolCallingType { ProgrammaticToolCalling = "programmatic_tool_calling" } }
model! { ResponseProgrammaticToolCalling {
    required { r#type: ResponseProgrammaticToolCallingType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseImageGenerationType { ImageGeneration = "image_generation" } }
literals! { ResponseImageGenerationAction { Generate = "generate", Edit = "edit", Auto = "auto" } }
literals! { ResponseImageGenerationBackground { Transparent = "transparent", Opaque = "opaque", Auto = "auto" } }
literals! { ResponseImageGenerationInputFidelity { High = "high", Low = "low" } }
model! { ResponseImageGenerationInputImageMask {
    required { }
    optional { file_id: String, image_url: String }
    nullable { }
    required_nullable { }
} }
literals! { ResponseImageGenerationModeration { Auto = "auto", Low = "low" } }
literals! { ResponseImageGenerationOutputFormat { Png = "png", Webp = "webp", Jpeg = "jpeg" } }
literals! { ResponseImageGenerationQuality { Low = "low", Medium = "medium", High = "high", Xhigh = "xhigh", Max = "max", Auto = "auto" } }
model! { ResponseImageGeneration {
    required { r#type: ResponseImageGenerationType }
    optional { action: ResponseImageGenerationAction, background: ResponseImageGenerationBackground, input_image_mask: ResponseImageGenerationInputImageMask, model: String, moderation: ResponseImageGenerationModeration, output_compression: i64 [min: Some(0), max: Some(100)], output_format: ResponseImageGenerationOutputFormat, partial_images: i64 [min: Some(0), max: Some(3)], quality: ResponseImageGenerationQuality, size: String }
    nullable { input_fidelity: ResponseImageGenerationInputFidelity }
    required_nullable { }
} }
literals! { ResponseLocalShellType { LocalShell = "local_shell" } }
model! { ResponseLocalShell {
    required { r#type: ResponseLocalShellType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseShellType { Shell = "shell" } }
literals! { ResponseContainerAutoType { ContainerAuto = "container_auto" } }
literals! { ResponseSkillReferenceType { SkillReference = "skill_reference" } }
model! { ResponseSkillReference {
    required { skill_id: String [min_len: Some(1), max_len: Some(64)], r#type: ResponseSkillReferenceType }
    optional { version: String }
    nullable { }
    required_nullable { }
} }
literals! { ResponseInlineSkillSourceMediaType { ApplicationZip = "application/zip" } }
literals! { ResponseInlineSkillSourceType { Base64 = "base64" } }
model! { ResponseInlineSkillSource {
    required { data: String [min_len: Some(1), max_len: Some(70_254_592)], media_type: ResponseInlineSkillSourceMediaType, r#type: ResponseInlineSkillSourceType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseInlineSkillType { Inline = "inline" } }
model! { ResponseInlineSkill {
    required { description: String, name: String, source: ResponseInlineSkillSource, r#type: ResponseInlineSkillType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseContainerAutoSkillsEntry { SkillReference(ResponseSkillReference), InlineSkill(ResponseInlineSkill) } }
model! { ResponseContainerAuto {
    required { r#type: ResponseContainerAutoType }
    optional { file_ids: Vec<String> [max_items: Some(50)], network_policy: ResponseCodeInterpreterToolAutoNetworkPolicy, skills: Vec<ResponseContainerAutoSkillsEntry> [max_items: Some(200)] }
    nullable { memory_limit: ResponseCodeInterpreterToolAutoMemoryLimit }
    required_nullable { }
} }
literals! { ResponseLocalEnvironmentType { Local = "local" } }
model! { ResponseLocalSkill {
    required { description: String, name: String, path: String }
    optional { }
    nullable { }
    required_nullable { }
} }
model! { ResponseLocalEnvironment {
    required { r#type: ResponseLocalEnvironmentType }
    optional { skills: Vec<ResponseLocalSkill> [max_items: Some(200)] }
    nullable { }
    required_nullable { }
} }
literals! { ResponseContainerReferenceType { ContainerReference = "container_reference" } }
model! { ResponseContainerReference {
    required { container_id: String, r#type: ResponseContainerReferenceType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseShellEnvironment { ContainerAuto(ResponseContainerAuto), LocalEnvironment(ResponseLocalEnvironment), ContainerReference(ResponseContainerReference) } }
model! { ResponseShell {
    required { r#type: ResponseShellType }
    optional { }
    nullable { allowed_callers: Vec<ResponseFunctionAllowedCallersEntry> [min_items: Some(1)], environment: ResponseShellEnvironment }
    required_nullable { }
} }
literals! { ResponseCustomType { Custom = "custom" } }
literals! { ResponseTextType { Text = "text" } }
model! { ResponseText {
    required { r#type: ResponseTextType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseGrammarSyntax { Lark = "lark", Regex = "regex" } }
literals! { ResponseGrammarType { Grammar = "grammar" } }
model! { ResponseGrammar {
    required { definition: String, syntax: ResponseGrammarSyntax, r#type: ResponseGrammarType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseCustomToolInputFormat { Text(ResponseText), Grammar(ResponseGrammar) } }
model! { ResponseCustom {
    required { name: String, r#type: ResponseCustomType }
    optional { r#async: bool, defer_loading: bool, description: String, format: ResponseCustomToolInputFormat }
    nullable { allowed_callers: Vec<ResponseFunctionAllowedCallersEntry> [min_items: Some(1)] }
    required_nullable { }
} }
model! { ResponseNamespaceFunction {
    required { name: String [min_len: Some(1), max_len: Some(128), identifier: true], r#type: ResponseFunctionType }
    optional { r#async: bool, defer_loading: bool }
    nullable { allowed_callers: Vec<ResponseFunctionAllowedCallersEntry> [min_items: Some(1)], description: String, output_schema: BTreeMap<String, Value>, parameters: BTreeMap<String, Value>, strict: bool }
    required_nullable { }
} }
union! { ResponseNamespaceToolsEntry { Function(ResponseNamespaceFunction), Custom(ResponseCustom) } }
literals! { ResponseNamespaceType { Namespace = "namespace" } }
model! { ResponseNamespace {
    required { description: String, name: String [min_len: Some(1)], tools: Vec<ResponseNamespaceToolsEntry> [min_items: Some(1)], r#type: ResponseNamespaceType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseToolSearchType { ToolSearch = "tool_search" } }
model! { ResponseToolSearch {
    required { r#type: ResponseToolSearchType }
    optional { execution: ResponseToolSearchCallExecution }
    nullable { description: String, parameters: BTreeMap<String, Value> }
    required_nullable { }
} }
literals! { ResponseWebSearchPreviewType { WebSearchPreview = "web_search_preview", WebSearchPreview20250311 = "web_search_preview_2025_03_11" } }
literals! { ResponseWebSearchPreviewSearchContentTypesEntry { Text = "text", Image = "image" } }
model! { ResponseWebSearchPreviewUserLocation {
    required { r#type: ResponseWebSearchUserLocationType }
    optional { }
    nullable { city: String, country: String, region: String, timezone: String }
    required_nullable { }
} }
model! { ResponseWebSearchPreview {
    required { r#type: ResponseWebSearchPreviewType }
    optional { search_content_types: Vec<ResponseWebSearchPreviewSearchContentTypesEntry>, search_context_size: ResponseWebSearchSearchContextSize }
    nullable { user_location: ResponseWebSearchPreviewUserLocation }
    required_nullable { }
} }
literals! { ResponseApplyPatchType { ApplyPatch = "apply_patch" } }
model! { ResponseApplyPatch {
    required { r#type: ResponseApplyPatchType }
    optional { }
    nullable { allowed_callers: Vec<ResponseFunctionAllowedCallersEntry> [min_items: Some(1)] }
    required_nullable { }
} }
union! {
    /// Tool descriptions embedded in the shared history/tool-search item schema.
    ///
    /// This type is not Live's session tool-registration configuration: only
    /// function and web-search registration is supported there.
    ResponseSharedTool { Function(ResponseFunction), FileSearch(ResponseFileSearch), Computer(ResponseComputer), ComputerUsePreview(ResponseComputerUsePreview), WebSearch(ResponseWebSearch), Mcp(ResponseMcp), CodeInterpreter(ResponseCodeInterpreter), ProgrammaticToolCalling(ResponseProgrammaticToolCalling), ImageGeneration(ResponseImageGeneration), LocalShell(ResponseLocalShell), Shell(ResponseShell), Custom(ResponseCustom), Namespace(ResponseNamespace), ToolSearch(ResponseToolSearch), WebSearchPreview(ResponseWebSearchPreview), ApplyPatch(ResponseApplyPatch) }
}
literals! { ResponseToolSearchOutputType { ToolSearchOutput = "tool_search_output" } }
model! { ResponseToolSearchOutput {
    required { tools: Vec<ResponseSharedTool>, r#type: ResponseToolSearchOutputType }
    optional { execution: ResponseToolSearchCallExecution }
    nullable { id: String, call_id: String [min_len: Some(1), max_len: Some(64)], status: ResponseMessageStatus }
    required_nullable { }
} }
literals! { ResponseAdditionalToolsRole { Developer = "developer" } }
literals! { ResponseAdditionalToolsType { AdditionalTools = "additional_tools" } }
model! { ResponseAdditionalTools {
    required { role: ResponseAdditionalToolsRole, tools: Vec<ResponseSharedTool>, r#type: ResponseAdditionalToolsType }
    optional { }
    nullable { id: String }
    required_nullable { }
} }
literals! { ResponseConfigurationUpdateType { ConfigurationUpdate = "configuration_update" } }
literals! { ResponseReasoningEffort { None = "none", Minimal = "minimal", Low = "low", Medium = "medium", High = "high", Xhigh = "xhigh", Max = "max" } }
model! { ResponseConfigurationUpdateReasoning {
    required { }
    optional { }
    nullable { effort: ResponseReasoningEffort }
    required_nullable { }
} }
model! { ResponseConfigurationUpdate {
    required { r#type: ResponseConfigurationUpdateType }
    optional { reasoning: ResponseConfigurationUpdateReasoning }
    nullable { id: String }
    required_nullable { }
} }
literals! { ResponseSummaryTextContentType { SummaryText = "summary_text" } }
model! { ResponseSummaryTextContent {
    required { text: String, r#type: ResponseSummaryTextContentType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseReasoningType { Reasoning = "reasoning" } }
literals! { ResponseReasoningContentEntryType { ReasoningText = "reasoning_text" } }
model! { ResponseReasoningContentEntry {
    required { text: String, r#type: ResponseReasoningContentEntryType }
    optional { }
    nullable { }
    required_nullable { }
} }
model! { ResponseReasoning {
    required { id: String, summary: Vec<ResponseSummaryTextContent>, r#type: ResponseReasoningType }
    optional { content: Vec<ResponseReasoningContentEntry>, status: ResponseMessageStatus }
    nullable { encrypted_content: String }
    required_nullable { }
} }
literals! { ResponseCompactionType { Compaction = "compaction" } }
model! { ResponseCompaction {
    required { encrypted_content: String [max_len: Some(20_971_520)], r#type: ResponseCompactionType }
    optional { }
    nullable { id: String }
    required_nullable { }
} }
literals! { ResponseImageGenerationCallStatus { InProgress = "in_progress", Completed = "completed", Generating = "generating", Failed = "failed" } }
literals! { ResponseImageGenerationCallType { ImageGenerationCall = "image_generation_call" } }
model! { ResponseImageGenerationCall {
    required { id: String, status: ResponseImageGenerationCallStatus, r#type: ResponseImageGenerationCallType }
    optional { }
    nullable { action: ResponseImageGenerationAction, background: ResponseImageGenerationBackground, output_format: ResponseImageGenerationOutputFormat, quality: ResponseImageGenerationQuality, revised_prompt: String, size: String }
    required_nullable { result: String }
} }
literals! { ResponseLogsType { Logs = "logs" } }
model! { ResponseLogs {
    required { logs: String, r#type: ResponseLogsType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseImageType { Image = "image" } }
model! { ResponseImage {
    required { r#type: ResponseImageType, url: String [uri: true] }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseCodeInterpreterCallOutputsEntry { Logs(ResponseLogs), Image(ResponseImage) } }
literals! { ResponseCodeInterpreterCallStatus { InProgress = "in_progress", Completed = "completed", Incomplete = "incomplete", Interpreting = "interpreting", Failed = "failed" } }
literals! { ResponseCodeInterpreterCallType { CodeInterpreterCall = "code_interpreter_call" } }
model! { ResponseCodeInterpreterCall {
    required { id: String, container_id: String, status: ResponseCodeInterpreterCallStatus, r#type: ResponseCodeInterpreterCallType }
    optional { }
    nullable { }
    required_nullable { code: String, outputs: Vec<ResponseCodeInterpreterCallOutputsEntry> }
} }
literals! { ResponseLocalShellCallActionType { Exec = "exec" } }
model! { ResponseLocalShellCallAction {
    required { command: Vec<String>, env: BTreeMap<String, String>, r#type: ResponseLocalShellCallActionType }
    optional { }
    nullable { timeout_ms: i64, user: String, working_directory: String }
    required_nullable { }
} }
literals! { ResponseLocalShellCallType { LocalShellCall = "local_shell_call" } }
model! { ResponseLocalShellCall {
    required { id: String, action: ResponseLocalShellCallAction, call_id: String, status: ResponseMessageStatus, r#type: ResponseLocalShellCallType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseLocalShellCallOutputType { LocalShellCallOutput = "local_shell_call_output" } }
model! { ResponseLocalShellCallOutput {
    required { id: String, output: String, r#type: ResponseLocalShellCallOutputType }
    optional { }
    nullable { status: ResponseMessageStatus }
    required_nullable { }
} }
model! { ResponseShellCallAction {
    required { commands: Vec<String> }
    optional { }
    nullable { max_output_length: i64, timeout_ms: i64 }
    required_nullable { }
} }
literals! { ResponseShellCallType { ShellCall = "shell_call" } }
union! { ResponseShellCallEnvironment { LocalEnvironment(ResponseLocalEnvironment), ContainerReference(ResponseContainerReference) } }
model! { ResponseShellCall {
    required { action: ResponseShellCallAction, call_id: String [min_len: Some(1), max_len: Some(64)], r#type: ResponseShellCallType }
    optional { }
    nullable { id: String, caller: ResponseToolCaller, environment: ResponseShellCallEnvironment, status: ResponseMessageStatus }
    required_nullable { }
} }
literals! { ResponseTimeoutType { Timeout = "timeout" } }
model! { ResponseTimeout {
    required { r#type: ResponseTimeoutType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseExitType { Exit = "exit" } }
model! { ResponseExit {
    required { exit_code: i64, r#type: ResponseExitType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseFunctionShellCallOutputContentOutcome { Timeout(ResponseTimeout), Exit(ResponseExit) } }
model! { ResponseFunctionShellCallOutputContent {
    required { outcome: ResponseFunctionShellCallOutputContentOutcome, stderr: String [max_len: Some(10_485_760)], stdout: String [max_len: Some(10_485_760)] }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseShellCallOutputType { ShellCallOutput = "shell_call_output" } }
model! { ResponseShellCallOutput {
    required { call_id: String [min_len: Some(1), max_len: Some(64)], output: Vec<ResponseFunctionShellCallOutputContent>, r#type: ResponseShellCallOutputType }
    optional { }
    nullable { id: String, caller: ResponseToolCaller, max_output_length: i64, status: ResponseMessageStatus }
    required_nullable { }
} }
literals! { ResponseCreateFileType { CreateFile = "create_file" } }
model! { ResponseCreateFile {
    required { diff: String [max_len: Some(10_485_760)], path: String [min_len: Some(1)], r#type: ResponseCreateFileType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseDeleteFileType { DeleteFile = "delete_file" } }
model! { ResponseDeleteFile {
    required { path: String [min_len: Some(1)], r#type: ResponseDeleteFileType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseUpdateFileType { UpdateFile = "update_file" } }
model! { ResponseUpdateFile {
    required { diff: String [max_len: Some(10_485_760)], path: String [min_len: Some(1)], r#type: ResponseUpdateFileType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseApplyPatchCallOperation { CreateFile(ResponseCreateFile), DeleteFile(ResponseDeleteFile), UpdateFile(ResponseUpdateFile) } }
literals! { ResponseApplyPatchCallStatus { InProgress = "in_progress", Completed = "completed" } }
literals! { ResponseApplyPatchCallType { ApplyPatchCall = "apply_patch_call" } }
model! { ResponseApplyPatchCall {
    required { call_id: String [min_len: Some(1), max_len: Some(64)], operation: ResponseApplyPatchCallOperation, status: ResponseApplyPatchCallStatus, r#type: ResponseApplyPatchCallType }
    optional { }
    nullable { id: String, caller: ResponseToolCaller }
    required_nullable { }
} }
literals! { ResponseApplyPatchCallOutputStatus { Completed = "completed", Failed = "failed" } }
literals! { ResponseApplyPatchCallOutputType { ApplyPatchCallOutput = "apply_patch_call_output" } }
model! { ResponseApplyPatchCallOutput {
    required { call_id: String [min_len: Some(1), max_len: Some(64)], status: ResponseApplyPatchCallOutputStatus, r#type: ResponseApplyPatchCallOutputType }
    optional { }
    nullable { id: String, caller: ResponseToolCaller, output: String [max_len: Some(10_485_760)] }
    required_nullable { }
} }
model! { ResponseMcpListToolsToolsEntry {
    required { input_schema: BTreeMap<String, Value>, name: String }
    optional { }
    nullable { annotations: BTreeMap<String, Value>, description: String }
    required_nullable { }
} }
literals! { ResponseMcpListToolsType { McpListTools = "mcp_list_tools" } }
model! { ResponseMcpListTools {
    required { id: String, server_label: String, tools: Vec<ResponseMcpListToolsToolsEntry>, r#type: ResponseMcpListToolsType }
    optional { }
    nullable { error: String }
    required_nullable { }
} }
literals! { ResponseMcpApprovalRequestType { McpApprovalRequest = "mcp_approval_request" } }
model! { ResponseMcpApprovalRequest {
    required { id: String, arguments: String, name: String, server_label: String, r#type: ResponseMcpApprovalRequestType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseMcpApprovalResponseType { McpApprovalResponse = "mcp_approval_response" } }
model! { ResponseMcpApprovalResponse {
    required { approval_request_id: String, approve: bool, r#type: ResponseMcpApprovalResponseType }
    optional { }
    nullable { id: String, reason: String }
    required_nullable { }
} }
literals! { ResponseMcpCallType { McpCall = "mcp_call" } }
literals! { ResponseMcpProtocolErrorType { McpProtocolError = "mcp_protocol_error" } }
model! { ResponseMcpProtocolError {
    required { code: i64, message: String, r#type: ResponseMcpProtocolErrorType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseMcpToolExecutionErrorType { McpToolExecutionError = "mcp_tool_execution_error" } }
model! { ResponseMcpToolExecutionError {
    required { content: Value, r#type: ResponseMcpToolExecutionErrorType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseHttpErrorType { HttpError = "http_error" } }
model! { ResponseHttpError {
    required { code: i64, message: String, r#type: ResponseHttpErrorType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! { ResponseMcpToolCallError { McpProtocolError(ResponseMcpProtocolError), McpToolExecutionError(ResponseMcpToolExecutionError), HttpError(ResponseHttpError) } }
literals! { ResponseMcpCallStatus { InProgress = "in_progress", Completed = "completed", Incomplete = "incomplete", Calling = "calling", Failed = "failed" } }
model! { ResponseMcpCall {
    required { id: String, arguments: String, name: String, server_label: String, r#type: ResponseMcpCallType }
    optional { status: ResponseMcpCallStatus }
    nullable { approval_request_id: String, error: ResponseMcpToolCallError, output: String }
    required_nullable { }
} }
literals! { ResponseCustomToolCallOutputType { CustomToolCallOutput = "custom_tool_call_output" } }
model! { ResponseCustomToolCallOutput {
    required { call_id: String, output: ResponseEasyInputMessageContent, r#type: ResponseCustomToolCallOutputType }
    optional { id: String }
    nullable { caller: ResponseToolCaller }
    required_nullable { }
} }
literals! { ResponseCustomToolCallType { CustomToolCall = "custom_tool_call" } }
model! { ResponseCustomToolCall {
    required { call_id: String, input: String, name: String, r#type: ResponseCustomToolCallType }
    optional { id: String, r#async: bool, namespace: String }
    nullable { caller: ResponseFunctionCallCaller }
    required_nullable { }
} }
literals! { ResponseCompactionTriggerType { CompactionTrigger = "compaction_trigger" } }
model! { ResponseCompactionTrigger {
    required { r#type: ResponseCompactionTriggerType }
    optional { }
    nullable { id: String }
    required_nullable { }
} }
literals! { ResponseItemReferenceType { ItemReference = "item_reference" } }
model! { ResponseItemReference {
    required { id: String }
    optional { }
    nullable { r#type: ResponseItemReferenceType }
    required_nullable { }
} }
model! { ResponseProgramItem {
    required { id: String, call_id: String [min_len: Some(1), max_len: Some(64)], code: String [max_len: Some(10_485_760)], fingerprint: String [max_len: Some(10_485_760)], r#type: ResponseProgramType }
    optional { }
    nullable { }
    required_nullable { }
} }
literals! { ResponseProgramOutputStatus { Completed = "completed", Incomplete = "incomplete" } }
literals! { ResponseProgramOutputType { ProgramOutput = "program_output" } }
model! { ResponseProgramOutput {
    required { id: String, call_id: String [min_len: Some(1), max_len: Some(64)], result: String [max_len: Some(10_485_760)], status: ResponseProgramOutputStatus, r#type: ResponseProgramOutputType }
    optional { }
    nullable { }
    required_nullable { }
} }
union! {
    /// All 33 alternatives accepted by the shared `response.item.create` schema.
    /// Start with [`Self::user_text`], [`Self::user_image`], or
    /// [`Self::function_call_output`] for common application paths.
    ///
    /// Overlapping easy-message/message representations decode to the first
    /// matching alternative without losing any wire fields.
    ResponseInputItem {
    EasyInputMessage(ResponseEasyInputMessage), Message(ResponseMessage),
    OutputMessage(ResponseOutputMessage), FileSearchCall(ResponseFileSearchCall),
    ComputerCall(ResponseComputerCall), ComputerCallOutput(ResponseComputerCallOutput),
    WebSearchCall(ResponseWebSearchCall), FunctionCall(ResponseFunctionCall),
    FunctionCallOutput(ResponseFunctionCallOutput), ToolSearchCall(ResponseToolSearchCall),
    ToolSearchOutput(ResponseToolSearchOutput), AdditionalTools(ResponseAdditionalTools),
    ConfigurationUpdate(ResponseConfigurationUpdate), Reasoning(ResponseReasoning),
    Compaction(ResponseCompaction), ImageGenerationCall(ResponseImageGenerationCall),
    CodeInterpreterCall(ResponseCodeInterpreterCall), LocalShellCall(ResponseLocalShellCall),
    LocalShellCallOutput(ResponseLocalShellCallOutput), ShellCall(ResponseShellCall),
    ShellCallOutput(ResponseShellCallOutput), ApplyPatchCall(ResponseApplyPatchCall),
    ApplyPatchCallOutput(ResponseApplyPatchCallOutput), McpListTools(ResponseMcpListTools),
    McpApprovalRequest(ResponseMcpApprovalRequest), McpApprovalResponse(ResponseMcpApprovalResponse),
    McpCall(ResponseMcpCall), CustomToolCallOutput(ResponseCustomToolCallOutput),
    CustomToolCall(ResponseCustomToolCall), CompactionTrigger(ResponseCompactionTrigger),
    ItemReference(ResponseItemReference), Program(ResponseProgramItem), ProgramOutput(ResponseProgramOutput)
} }

impl ResponseInputItem {
    /// A user message containing plain text.
    #[must_use]
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::EasyInputMessage(ResponseEasyInputMessage {
            content: ResponseEasyInputMessageContent::TextInput(text.into()),
            role: ResponseEasyInputMessageRole::User,
            r#type: Some(ResponseEasyInputMessageType::Message),
            phase: Field::Absent,
        })
    }

    /// A user message containing an image URL (including a data URL).
    #[must_use]
    pub fn user_image(image_url: impl Into<String>, detail: ResponseImageDetail) -> Self {
        Self::EasyInputMessage(ResponseEasyInputMessage {
            content: ResponseEasyInputMessageContent::ListResponseInputContent(vec![
                ResponseInputContent::InputImage(ResponseInputImage {
                    detail,
                    r#type: ResponseInputImageType::InputImage,
                    file_id: Field::Absent,
                    image_url: Field::Value(image_url.into()),
                    prompt_cache_breakpoint: None,
                }),
            ]),
            role: ResponseEasyInputMessageRole::User,
            r#type: Some(ResponseEasyInputMessageType::Message),
            phase: Field::Absent,
        })
    }

    /// An application's result for an authorized, completed function call.
    ///
    /// This only constructs an item. Submitting it does not continue the backend;
    /// submit all pending results before explicitly sending `response.create`.
    #[must_use]
    pub fn function_call_output(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self::FunctionCallOutput(ResponseFunctionCallOutput {
            output: ResponseFunctionOutput::Text(output.into()),
            r#type: ResponseFunctionCallOutputType::FunctionCallOutput,
            id: Field::Absent,
            call_id: Field::Value(call_id.into()),
            caller: Field::Absent,
            name: Field::Absent,
            namespace: Field::Absent,
            status: Field::Absent,
        })
    }

    /// Validate documented string, URI, and numeric constraints recursively.
    ///
    /// This does not estimate tokens, enforce model-dependent capabilities, execute
    /// tools, or grant registration permissions.
    ///
    /// # Errors
    ///
    /// Returns a redacted field path when a documented bound is violated.
    pub fn validate(&self) -> Result<(), String> {
        self.check(&Bounds::default())
    }
}

/// The finished function-call fields, not an authorization to run the function.
///
/// `args` is the exact final wire `arguments` string. Applications must parse and
/// validate it and enforce permissions before executing any operation.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    #[serde(rename = "arguments")]
    pub args: String,
    pub call_id: String,
    #[serde(default, deserialize_with = "nonnull")]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "nonnull")]
    pub status: Option<ResponseMessageStatus>,
}

redacted_debug!(FunctionCall);

/// An inbound output item. Unrecognized item types retain their complete object.
#[derive(Clone, PartialEq, Eq)]
pub enum ResponseEventItem {
    FunctionCall(FunctionCall),
    Other {
        item_type: String,
        raw: Map<String, Value>,
    },
}

redacted_debug!(ResponseEventItem);

impl<'de> Deserialize<'de> for ResponseEventItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let item_type = discriminator(&value).map_err(serde::de::Error::custom)?;
        if item_type == "function_call" {
            return serde_json::from_value(value)
                .map(Self::FunctionCall)
                .map_err(serde::de::Error::custom);
        }
        let item_type = item_type.to_owned();
        let Value::Object(raw) = value else {
            return Err(serde::de::Error::custom("expected an output item object"));
        };
        Ok(Self::Other { item_type, raw })
    }
}

/// Status values in a delegated Responses lifecycle snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseLifecycleStatus {
    Completed,
    Failed,
    InProgress,
    Cancelled,
    Queued,
    Incomplete,
}

/// Routing and output projection of a Responses lifecycle snapshot.
///
/// Additional inbound snapshot settings are intentionally ignored. Live clears
/// `output` to `[]` in forwarded lifecycle events, including completion; these
/// arrays must not replace calls collected from `response.output_item.done`.
#[derive(Clone, PartialEq, Deserialize)]
pub struct ResponseSnapshot {
    pub id: String,
    /// UNIX seconds; JSON strings are not accepted.
    pub created_at: f64,
    #[serde(default)]
    pub completed_at: Field<f64>,
    pub output: Vec<ResponseEventItem>,
    #[serde(default, deserialize_with = "nonnull")]
    pub status: Option<ResponseLifecycleStatus>,
}

redacted_debug!(ResponseSnapshot);

/// Lifecycle event names modeled by the delegated event decoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseLifecycleKind {
    Created,
    InProgress,
    Completed,
    Failed,
    Incomplete,
    Queued,
}

/// A streaming token probability. Unknown inbound fields are ignored.
#[derive(Clone, PartialEq, Deserialize)]
pub struct ResponseStreamLogprob {
    pub token: String,
    pub logprob: f64,
    #[serde(default, deserialize_with = "nonnull")]
    pub top_logprobs: Option<Vec<ResponseStreamTopLogprob>>,
}

redacted_debug!(ResponseStreamLogprob);

/// A streaming alternative token probability.
#[derive(Clone, PartialEq, Deserialize)]
pub struct ResponseStreamTopLogprob {
    #[serde(default, deserialize_with = "nonnull")]
    pub token: Option<String>,
    #[serde(default, deserialize_with = "nonnull")]
    pub logprob: Option<f64>,
}

redacted_debug!(ResponseStreamTopLogprob);

/// A nested Responses event, not a top-level Live event.
///
/// Lifecycle, output-item, function-argument, and output-text events are typed.
/// Other event types retain their raw map for forward compatibility. Malformed
/// modeled events return a decoding error and never become `Unknown`.
#[derive(Clone, PartialEq)]
pub enum ResponseEvent {
    Lifecycle {
        kind: ResponseLifecycleKind,
        sequence_number: i64,
        response: ResponseSnapshot,
    },
    OutputItemAdded {
        sequence_number: i64,
        output_index: i64,
        item: ResponseEventItem,
    },
    OutputItemDone {
        sequence_number: i64,
        output_index: i64,
        item: ResponseEventItem,
    },
    FunctionCallArgumentsDelta {
        sequence_number: i64,
        output_index: i64,
        item_id: String,
        delta: String,
    },
    FunctionCallArgumentsDone {
        sequence_number: i64,
        output_index: i64,
        item_id: String,
        arguments: String,
    },
    OutputTextDelta {
        sequence_number: i64,
        output_index: i64,
        content_index: i64,
        item_id: String,
        delta: String,
        logprobs: Vec<ResponseStreamLogprob>,
    },
    OutputTextDone {
        sequence_number: i64,
        output_index: i64,
        content_index: i64,
        item_id: String,
        text: String,
        logprobs: Vec<ResponseStreamLogprob>,
    },
    Unknown {
        event_type: String,
        raw: Map<String, Value>,
    },
}

redacted_debug!(ResponseEvent);

#[derive(Deserialize)]
struct LifecycleWire {
    sequence_number: i64,
    response: ResponseSnapshot,
}

#[derive(Deserialize)]
struct OutputItemWire {
    sequence_number: i64,
    output_index: i64,
    item: ResponseEventItem,
}

#[derive(Deserialize)]
struct ArgumentsDeltaWire {
    sequence_number: i64,
    output_index: i64,
    item_id: String,
    delta: String,
}

#[derive(Deserialize)]
struct ArgumentsDoneWire {
    sequence_number: i64,
    output_index: i64,
    item_id: String,
    arguments: String,
}

#[derive(Deserialize)]
struct TextDeltaWire {
    sequence_number: i64,
    output_index: i64,
    content_index: i64,
    item_id: String,
    delta: String,
    logprobs: Vec<ResponseStreamLogprob>,
}

#[derive(Deserialize)]
struct TextDoneWire {
    sequence_number: i64,
    output_index: i64,
    content_index: i64,
    item_id: String,
    text: String,
    logprobs: Vec<ResponseStreamLogprob>,
}

fn discriminator(value: &Value) -> Result<&str, serde_json::Error> {
    value
        .as_object()
        .and_then(|map| map.get("type"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            <serde_json::Error as serde::de::Error>::custom("expected an object with a string type")
        })
}

impl ResponseEvent {
    /// Decode only the `event` object inside Live's `response.event` envelope.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing/non-string discriminator or malformed
    /// fields of a modeled event. Unknown event names are preserved unchanged.
    pub fn decode(value: Value) -> Result<Self, serde_json::Error> {
        let event_type = discriminator(&value)?;
        let lifecycle = match event_type {
            "response.created" => Some(ResponseLifecycleKind::Created),
            "response.in_progress" => Some(ResponseLifecycleKind::InProgress),
            "response.completed" => Some(ResponseLifecycleKind::Completed),
            "response.failed" => Some(ResponseLifecycleKind::Failed),
            "response.incomplete" => Some(ResponseLifecycleKind::Incomplete),
            "response.queued" => Some(ResponseLifecycleKind::Queued),
            _ => None,
        };
        if let Some(kind) = lifecycle {
            let wire: LifecycleWire = serde_json::from_value(value)?;
            return Ok(Self::Lifecycle {
                kind,
                sequence_number: wire.sequence_number,
                response: wire.response,
            });
        }
        match event_type {
            "response.output_item.added" => {
                let wire: OutputItemWire = serde_json::from_value(value)?;
                Ok(Self::OutputItemAdded {
                    sequence_number: wire.sequence_number,
                    output_index: wire.output_index,
                    item: wire.item,
                })
            }
            "response.output_item.done" => {
                let wire: OutputItemWire = serde_json::from_value(value)?;
                Ok(Self::OutputItemDone {
                    sequence_number: wire.sequence_number,
                    output_index: wire.output_index,
                    item: wire.item,
                })
            }
            "response.function_call_arguments.delta" => {
                let wire: ArgumentsDeltaWire = serde_json::from_value(value)?;
                Ok(Self::FunctionCallArgumentsDelta {
                    sequence_number: wire.sequence_number,
                    output_index: wire.output_index,
                    item_id: wire.item_id,
                    delta: wire.delta,
                })
            }
            "response.function_call_arguments.done" => {
                let wire: ArgumentsDoneWire = serde_json::from_value(value)?;
                Ok(Self::FunctionCallArgumentsDone {
                    sequence_number: wire.sequence_number,
                    output_index: wire.output_index,
                    item_id: wire.item_id,
                    arguments: wire.arguments,
                })
            }
            "response.output_text.delta" => {
                let wire: TextDeltaWire = serde_json::from_value(value)?;
                Ok(Self::OutputTextDelta {
                    sequence_number: wire.sequence_number,
                    output_index: wire.output_index,
                    content_index: wire.content_index,
                    item_id: wire.item_id,
                    delta: wire.delta,
                    logprobs: wire.logprobs,
                })
            }
            "response.output_text.done" => {
                let wire: TextDoneWire = serde_json::from_value(value)?;
                Ok(Self::OutputTextDone {
                    sequence_number: wire.sequence_number,
                    output_index: wire.output_index,
                    content_index: wire.content_index,
                    item_id: wire.item_id,
                    text: wire.text,
                    logprobs: wire.logprobs,
                })
            }
            _ => {
                let event_type = event_type.to_owned();
                let Value::Object(raw) = value else {
                    return Err(<serde_json::Error as serde::de::Error>::custom(
                        "expected a Responses event object",
                    ));
                };
                Ok(Self::Unknown { event_type, raw })
            }
        }
    }

    /// Return a finished function item only from `response.output_item.done`.
    ///
    /// Argument deltas/done events, added items, and lifecycle output snapshots
    /// never trigger this accessor. Explicitly incomplete items are not actionable.
    #[must_use]
    pub const fn completed_function_call(&self) -> Option<&FunctionCall> {
        match self {
            Self::OutputItemDone {
                item: ResponseEventItem::FunctionCall(call),
                ..
            } if matches!(call.status, None | Some(ResponseMessageStatus::Completed)) => Some(call),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for ResponseEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::decode(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A backend response identity and its available outer Live delegation scope.
///
/// `None` means the envelope omitted or explicitly cleared its delegation ID.
/// Such a lifecycle fact still has a response ID, but cannot establish ownership
/// of ID-less granular events. The original frame retains absent versus null.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResponseKey {
    pub delegation_id: Option<String>,
    pub response_id: String,
}

redacted_debug!(ResponseKey);

/// Attribution evidence, not permission to execute a completed function.
#[derive(Clone, PartialEq, Eq)]
pub enum ResponseAttribution {
    /// Explicit lifecycle identity, a previously bound item, or the sole open
    /// response within a known delegation.
    Owned(ResponseKey),
    /// No safe owner is known. The caller must retain or report this event.
    Unowned,
    /// Multiple responses could own the event; no last-active guess is made.
    Ambiguous(Vec<ResponseKey>),
}

redacted_debug!(ResponseAttribution);

#[derive(Clone, PartialEq, Eq)]
struct TrackedOutput {
    item: ResponseEventItem,
    done: bool,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct TrackedResponse {
    started: bool,
    terminal: Option<ResponseLifecycleKind>,
    uncertain: bool,
    items: BTreeMap<i64, TrackedOutput>,
    calls: Vec<FunctionCall>,
}

impl ResponseEventItem {
    fn id(&self) -> Option<&str> {
        match self {
            Self::FunctionCall(call) => call.id.as_deref(),
            Self::Other { raw, .. } => raw.get("id").and_then(Value::as_str),
        }
    }

    fn call_id(&self) -> Option<&str> {
        match self {
            Self::FunctionCall(call) => Some(&call.call_id),
            Self::Other { .. } => None,
        }
    }
}

impl ResponseEvent {
    fn item_id(&self) -> Option<&str> {
        match self {
            Self::OutputItemAdded { item, .. } | Self::OutputItemDone { item, .. } => item.id(),
            Self::FunctionCallArgumentsDelta { item_id, .. }
            | Self::FunctionCallArgumentsDone { item_id, .. }
            | Self::OutputTextDelta { item_id, .. }
            | Self::OutputTextDone { item_id, .. } => Some(item_id),
            _ => None,
        }
    }

    fn call_id(&self) -> Option<&str> {
        match self {
            Self::OutputItemAdded { item, .. } | Self::OutputItemDone { item, .. } => {
                item.call_id()
            }
            _ => None,
        }
    }
}

/// Track function facts and completion barriers by response and delegation.
///
/// Feed the complete stream in received order, including non-function items and
/// lifecycle events. A finished item does not finish its response or call batch.
/// [`Self::ready_calls`] requires `response.created`, a matching successful
/// terminal event, every observed item done, and no unresolved attribution.
/// Cleared lifecycle output arrays never erase collected calls.
///
/// Null/omitted scopes never select an arbitrary active response. An unowned
/// granular event makes currently open possible owners uncertain; ambiguity is
/// sticky until those responses are removed. Callers must handle the returned
/// attribution explicitly and report decode errors or stream loss with
/// [`Self::mark_uncertain`]. No events are buffered for speculative reassignment.
///
/// This helper never executes tools or continues work. State is retained until
/// [`Self::remove`]; applications must enforce their own retention limits.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct FunctionCallTracker {
    responses: BTreeMap<ResponseKey, TrackedResponse>,
}

redacted_debug!(FunctionCallTracker);

impl FunctionCallTracker {
    /// Observe a nested event with the envelope's available delegation ID.
    ///
    /// # Errors
    ///
    /// Rejects conflicting items/terminals or new items after a terminal event.
    /// Exact duplicate completed items are idempotent. Errors make the affected
    /// response uncertain; already collected facts remain available.
    pub fn observe(
        &mut self,
        delegation_id: Option<&str>,
        event: &ResponseEvent,
    ) -> Result<ResponseAttribution, String> {
        if let ResponseEvent::Lifecycle { kind, response, .. } = event {
            let key = ResponseKey {
                delegation_id: delegation_id.map(str::to_owned),
                response_id: response.id.clone(),
            };
            let state = self.responses.entry(key.clone()).or_default();
            let terminal = matches!(
                kind,
                ResponseLifecycleKind::Completed
                    | ResponseLifecycleKind::Failed
                    | ResponseLifecycleKind::Incomplete
            );
            if state.terminal.is_some_and(|previous| previous != *kind) {
                state.uncertain = true;
                return Err("conflicting lifecycle event after a response terminal".into());
            }
            if *kind == ResponseLifecycleKind::Created {
                state.started = true;
            }
            if terminal {
                state.terminal = Some(*kind);
            }
            return Ok(ResponseAttribution::Owned(key));
        }
        if matches!(event, ResponseEvent::Unknown { .. }) {
            return Ok(ResponseAttribution::Unowned);
        }
        let owners = self.possible_owners(delegation_id, event);
        let [key] = owners.as_slice() else {
            if owners.is_empty() {
                self.mark_uncertain(delegation_id);
                return Ok(ResponseAttribution::Unowned);
            }
            for key in &owners {
                self.responses
                    .get_mut(key)
                    .ok_or("missing tracked owner")?
                    .uncertain = true;
            }
            return Ok(ResponseAttribution::Ambiguous(owners));
        };
        let state = self.responses.get_mut(key).ok_or("missing tracked owner")?;
        if let Err(error) = Self::observe_item(state, event) {
            state.uncertain = true;
            return Err(error);
        }
        Ok(ResponseAttribution::Owned(key.clone()))
    }

    fn possible_owners(
        &self,
        delegation_id: Option<&str>,
        event: &ResponseEvent,
    ) -> Vec<ResponseKey> {
        let Some(scope) = delegation_id else {
            return Vec::new();
        };
        let scoped = || {
            self.responses
                .iter()
                .filter(|(key, _)| key.delegation_id.as_deref() == Some(scope))
        };
        let bound: Vec<_> = scoped()
            .filter(|(_, state)| {
                state.items.values().any(|output| {
                    event
                        .item_id()
                        .is_some_and(|id| output.item.id() == Some(id))
                        || event
                            .call_id()
                            .is_some_and(|id| output.item.call_id() == Some(id))
                })
            })
            .map(|(key, _)| key.clone())
            .collect();
        if !bound.is_empty() {
            return bound;
        }
        scoped()
            .filter(|(_, state)| state.terminal.is_none())
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn observe_item(state: &mut TrackedResponse, event: &ResponseEvent) -> Result<(), String> {
        let (index, item, done) = match event {
            ResponseEvent::OutputItemAdded {
                output_index, item, ..
            } => (*output_index, item, false),
            ResponseEvent::OutputItemDone {
                output_index, item, ..
            } => (*output_index, item, true),
            _ => return Ok(()),
        };
        if let Some(previous) = state.items.get(&index) {
            if previous.done {
                return if done && &previous.item == item {
                    Ok(())
                } else {
                    Err("conflicting output item after output_item.done".into())
                };
            }
            if previous.item.id() != item.id() || previous.item.call_id() != item.call_id() {
                return Err("conflicting output identities at the same output_index".into());
            }
        }
        if state.terminal.is_some() {
            return Err("new output item after a response terminal".into());
        }
        if state.items.iter().any(|(other_index, output)| {
            *other_index != index
                && (item.id().is_some_and(|id| output.item.id() == Some(id))
                    || item
                        .call_id()
                        .is_some_and(|id| output.item.call_id() == Some(id)))
        }) {
            return Err("duplicate output identity at different output indices".into());
        }
        if let Some(call) = event.completed_function_call() {
            state.calls.push(call.clone());
        }
        state.items.insert(
            index,
            TrackedOutput {
                item: item.clone(),
                done,
            },
        );
        Ok(())
    }

    /// Mark open possible owners incomplete after a lost or malformed event.
    /// `None` conservatively marks all open responses, not one guessed scope.
    pub fn mark_uncertain(&mut self, delegation_id: Option<&str>) {
        for (key, state) in &mut self.responses {
            if state.terminal.is_none()
                && delegation_id.is_none_or(|scope| key.delegation_id.as_deref() == Some(scope))
            {
                state.uncertain = true;
            }
        }
    }

    /// Collected completed items, which may still be an unfinished call batch.
    #[must_use]
    pub fn calls(&self, key: &ResponseKey) -> Option<&[FunctionCall]> {
        self.responses.get(key).map(|state| state.calls.as_slice())
    }

    /// A complete call set after this exact scoped response successfully ended.
    ///
    /// `Some(&[])` means a confirmed empty set. `None` means unknown, unfinished,
    /// failed/incomplete, unattributed, or otherwise uncertain, never empty.
    #[must_use]
    pub fn ready_calls(&self, key: &ResponseKey) -> Option<&[FunctionCall]> {
        let state = self.responses.get(key)?;
        (key.delegation_id.is_some()
            && state.started
            && !state.uncertain
            && state.terminal == Some(ResponseLifecycleKind::Completed)
            && state.items.values().all(|output| {
                output.done
                    && match &output.item {
                        ResponseEventItem::FunctionCall(call) => {
                            matches!(call.status, None | Some(ResponseMessageStatus::Completed))
                        }
                        ResponseEventItem::Other { .. } => true,
                    }
            }))
        .then_some(state.calls.as_slice())
    }

    /// The observed terminal kind for this exact response, independent of calls.
    #[must_use]
    pub fn terminal(&self, key: &ResponseKey) -> Option<ResponseLifecycleKind> {
        self.responses.get(key).and_then(|state| state.terminal)
    }

    /// Release facts only once late duplicates no longer need attribution.
    pub fn remove(&mut self, key: &ResponseKey) -> Option<Vec<FunctionCall>> {
        self.responses.remove(key).map(|state| state.calls)
    }
}
