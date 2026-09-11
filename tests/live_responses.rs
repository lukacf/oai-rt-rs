#![allow(clippy::too_many_lines)] // Schema fixture tables are intentionally exhaustive.

use oai_rt_rs::live::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::any::type_name;

fn roundtrip<T: DeserializeOwned + Serialize>(wire: &str) {
    let value: Value = serde_json::from_str(wire).expect("valid fixture JSON");
    let parsed: T = serde_json::from_value(value.clone())
        .unwrap_or_else(|error| panic!("{}: {error}", type_name::<T>()));
    assert_eq!(
        serde_json::to_value(parsed).expect("serialize typed fixture"),
        value,
        "{} must preserve every supplied key and value",
        type_name::<T>()
    );
}

fn shape<T: DeserializeOwned + Serialize + std::fmt::Debug>(
    wire: &str,
    optional: &[&str],
    nullable: &[&str],
) {
    roundtrip::<T>(wire);
    let value: Value = serde_json::from_str(wire).expect("valid object fixture");
    let parsed: T = serde_json::from_value(value.clone()).unwrap();
    assert!(
        !format!("{parsed:?}").contains("fixture"),
        "{} debug leaked fixture content",
        type_name::<T>()
    );
    memberships(type_name::<T>(), &value);
    let object = value.as_object().expect("fixture object");
    for key in object.keys() {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        let missing_result = serde_json::from_value::<T>(missing.clone());
        assert_eq!(
            missing_result.is_ok(),
            optional.contains(&key.as_str()),
            "{} omitted {key}",
            type_name::<T>()
        );
        if let Ok(parsed) = missing_result {
            assert_eq!(serde_json::to_value(parsed).unwrap(), missing);
        }
        let mut null = value.clone();
        null[key] = Value::Null;
        let null_result = serde_json::from_value::<T>(null.clone());
        assert_eq!(
            null_result.is_ok(),
            nullable.contains(&key.as_str()),
            "{} null {key}",
            type_name::<T>()
        );
        if let Ok(parsed) = null_result {
            assert_eq!(serde_json::to_value(parsed).unwrap(), null);
        }
    }
    let mut future = value;
    future["unrecognized_future_field"] = json!("must not silently discard");
    assert!(
        serde_json::from_value::<T>(future).is_err(),
        "{} unknown outbound key",
        type_name::<T>()
    );
}

fn literals<T: DeserializeOwned + Serialize>(values: &[&str]) {
    for value in values {
        roundtrip::<T>(&serde_json::to_string(value).unwrap());
        memberships(type_name::<T>(), &json!(value));
    }

    assert!(serde_json::from_value::<T>(json!("unrecognized_future_literal")).is_err());
    assert!(serde_json::from_value::<T>(Value::Null).is_err());
    assert!(serde_json::from_value::<T>(json!(1)).is_err());
}

fn union_value<T: DeserializeOwned + Serialize>(value: &Value) {
    let parsed: T = serde_json::from_value(value.clone())
        .unwrap_or_else(|error| panic!("{}: {error}", type_name::<T>()));
    assert_eq!(
        serde_json::to_value(parsed).unwrap(),
        *value,
        "{}",
        type_name::<T>()
    );
    if type_name::<T>() == type_name::<ResponseInputItem>() {
        serde_json::from_value::<ResponseInputItem>(value.clone())
            .unwrap()
            .validate()
            .unwrap();
    }
    memberships(type_name::<T>(), value);
}

// Every object and literal witness is also checked through every union that
// contains it. Array alternatives wrap the witness in an array first.
fn memberships(name: &str, value: &Value) {
    macro_rules! members {
            ($($union:ty => [$($direct:ty),*] [$($array:ty),*];)*) => {
                $(
                    let direct: &[&str] = &[$(type_name::<$direct>()),*];
                    if direct.contains(&name) {
                        union_value::<$union>(value);
                    }
                    let array: &[&str] = &[$(type_name::<$array>()),*];
                    if array.contains(&name) {
                        union_value::<$union>(&Value::Array(vec![value.clone()]));
                    }
                )*
            };
        }
    members! {
        ResponseInputItem => [ResponseEasyInputMessage, ResponseMessage, ResponseOutputMessage, ResponseFileSearchCall, ResponseComputerCall, ResponseComputerCallOutput, ResponseWebSearchCall, ResponseFunctionCall, ResponseFunctionCallOutput, ResponseToolSearchCall, ResponseToolSearchOutput, ResponseAdditionalTools, ResponseConfigurationUpdate, ResponseReasoning, ResponseCompaction, ResponseImageGenerationCall, ResponseCodeInterpreterCall, ResponseLocalShellCall, ResponseLocalShellCallOutput, ResponseShellCall, ResponseShellCallOutput, ResponseApplyPatchCall, ResponseApplyPatchCallOutput, ResponseMcpListTools, ResponseMcpApprovalRequest, ResponseMcpApprovalResponse, ResponseMcpCall, ResponseCustomToolCallOutput, ResponseCustomToolCall, ResponseCompactionTrigger, ResponseItemReference, ResponseProgramItem, ResponseProgramOutput] [];
        ResponseEasyInputMessageContent => [String] [ResponseInputContent];
        ResponseInputContent => [ResponseInputText, ResponseInputImage, ResponseInputFile] [];
        ResponseOutputMessageContentEntry => [ResponseOutputText, ResponseOutputRefusal] [];
        ResponseOutputTextAnnotationsEntry => [ResponseFileCitation, ResponseURLCitation, ResponseContainerFileCitation, ResponseFilePath] [];
        ResponseFileSearchCallResultsEntryAttributesValue => [String, f64, bool] [];
        ResponseComputerAction => [ResponseClick, ResponseDoubleClick, ResponseDrag, ResponseKeypress, ResponseMove, ResponseScreenshot, ResponseScroll, ResponseType, ResponseWait] [];
        ResponseWebSearchCallAction => [ResponseSearch, ResponseOpenPage, ResponseFindInPage] [];
        ResponseFunctionCallCaller => [ResponseDirect, ResponseProgram] [];
        ResponseFunctionOutput => [String] [ResponseToolOutputContent];
        ResponseToolOutputContent => [ResponseInputTextContent, ResponseInputImageContent, ResponseInputFileContent] [];
        ResponseToolCaller => [ResponseDirect, ResponseProgramCaller] [];
        ResponseSharedTool => [ResponseFunction, ResponseFileSearch, ResponseComputer, ResponseComputerUsePreview, ResponseWebSearch, ResponseMcp, ResponseCodeInterpreter, ResponseProgrammaticToolCalling, ResponseImageGeneration, ResponseLocalShell, ResponseShell, ResponseCustom, ResponseNamespace, ResponseToolSearch, ResponseWebSearchPreview, ResponseApplyPatch] [];
        ResponseFileSearchFilters => [ResponseComparisonFilter, ResponseCompoundFilter] [];
        ResponseComparisonFilterValue => [String, f64, bool] [ResponseFilterScalar];
        ResponseFilterScalar => [String, f64] [];
        ResponseCompoundFilterFiltersEntry => [ResponseComparisonFilter, Value] [];
        ResponseMcpAllowedTools => [ResponseMcpToolFilter] [String];
        ResponseMcpRequireApproval => [ResponseMcpToolApprovalFilter, ResponseMcpToolApprovalSetting] [];
        ResponseCodeInterpreterContainer => [String, ResponseCodeInterpreterToolAuto] [];
        ResponseCodeInterpreterToolAutoNetworkPolicy => [ResponseContainerNetworkPolicyDisabled, ResponseContainerNetworkPolicyAllowlist] [];
        ResponseShellEnvironment => [ResponseContainerAuto, ResponseLocalEnvironment, ResponseContainerReference] [];
        ResponseContainerAutoSkillsEntry => [ResponseSkillReference, ResponseInlineSkill] [];
        ResponseCustomToolInputFormat => [ResponseText, ResponseGrammar] [];
        ResponseNamespaceToolsEntry => [ResponseNamespaceFunction, ResponseCustom] [];
        ResponseCodeInterpreterCallOutputsEntry => [ResponseLogs, ResponseImage] [];
        ResponseShellCallEnvironment => [ResponseLocalEnvironment, ResponseContainerReference] [];
        ResponseFunctionShellCallOutputContentOutcome => [ResponseTimeout, ResponseExit] [];
        ResponseApplyPatchCallOperation => [ResponseCreateFile, ResponseDeleteFile, ResponseUpdateFile] [];
        ResponseMcpToolCallError => [ResponseMcpProtocolError, ResponseMcpToolExecutionError, ResponseHttpError] [];
    }
}

#[test]
fn primitive_union_alternatives() {
    memberships(type_name::<String>(), &json!("fixture"));
    memberships(type_name::<f64>(), &json!(1.25));
    memberships(type_name::<bool>(), &json!(true));
    for unknown in [
        Value::Null,
        json!({"future": [null, 1, "fixture"]}),
        json!(42),
    ] {
        memberships(type_name::<Value>(), &unknown);
    }
}

#[test]
fn every_documented_literal_and_unknown_literal_rejection() {
    literals::<ResponseInputTextType>(&["input_text"]);
    literals::<ResponseInputTextPromptCacheBreakpointMode>(&["explicit"]);
    literals::<ResponseImageDetail>(&["low", "high", "auto", "original"]);
    literals::<ResponseInputImageType>(&["input_image"]);
    literals::<ResponseInputFileType>(&["input_file"]);
    literals::<ResponseInputFileDetail>(&["auto", "low", "high"]);
    literals::<ResponseEasyInputMessageRole>(&["user", "assistant", "system", "developer"]);
    literals::<ResponseEasyInputMessagePhase>(&["commentary", "final_answer"]);
    literals::<ResponseEasyInputMessageType>(&["message"]);
    literals::<ResponseMessageRole>(&["user", "system", "developer"]);
    literals::<ResponseMessageStatus>(&["in_progress", "completed", "incomplete"]);
    literals::<ResponseFileCitationType>(&["file_citation"]);
    literals::<ResponseURLCitationType>(&["url_citation"]);
    literals::<ResponseContainerFileCitationType>(&["container_file_citation"]);
    literals::<ResponseFilePathType>(&["file_path"]);
    literals::<ResponseOutputTextType>(&["output_text"]);
    literals::<ResponseOutputRefusalType>(&["refusal"]);
    literals::<ResponseOutputMessageRole>(&["assistant"]);
    literals::<ResponseFileSearchCallStatus>(&[
        "in_progress",
        "searching",
        "completed",
        "incomplete",
        "failed",
    ]);
    literals::<ResponseFileSearchCallType>(&["file_search_call"]);
    literals::<ResponseComputerCallType>(&["computer_call"]);
    literals::<ResponseClickButton>(&["left", "right", "wheel", "back", "forward"]);
    literals::<ResponseClickType>(&["click"]);
    literals::<ResponseDoubleClickType>(&["double_click"]);
    literals::<ResponseDragType>(&["drag"]);
    literals::<ResponseKeypressType>(&["keypress"]);
    literals::<ResponseMoveType>(&["move"]);
    literals::<ResponseScreenshotType>(&["screenshot"]);
    literals::<ResponseScrollType>(&["scroll"]);
    literals::<ResponseTypeType>(&["type"]);
    literals::<ResponseWaitType>(&["wait"]);
    literals::<ResponseComputerToolCallOutputScreenshotType>(&["computer_screenshot"]);
    literals::<ResponseComputerCallOutputType>(&["computer_call_output"]);
    literals::<ResponseSearchType>(&["search"]);
    literals::<ResponseSearchSourcesEntryType>(&["url"]);
    literals::<ResponseOpenPageType>(&["open_page"]);
    literals::<ResponseFindInPageType>(&["find_in_page"]);
    literals::<ResponseWebSearchCallStatus>(&[
        "in_progress",
        "searching",
        "completed",
        "failed",
        "incomplete",
    ]);
    literals::<ResponseWebSearchCallType>(&["web_search_call"]);
    literals::<ResponseFunctionCallType>(&["function_call"]);
    literals::<ResponseDirectType>(&["direct"]);
    literals::<ResponseProgramType>(&["program"]);
    literals::<ResponseFunctionCallOutputType>(&["function_call_output"]);
    literals::<ResponseToolSearchCallType>(&["tool_search_call"]);
    literals::<ResponseToolSearchCallExecution>(&["server", "client"]);
    literals::<ResponseFunctionType>(&["function"]);
    literals::<ResponseFunctionAllowedCallersEntry>(&["direct", "programmatic"]);
    literals::<ResponseFileSearchType>(&["file_search"]);
    literals::<ResponseComparisonFilterType>(&["eq", "ne", "gt", "gte", "lt", "lte", "in", "nin"]);
    literals::<ResponseCompoundFilterType>(&["and", "or"]);
    literals::<ResponseFileSearchRankingOptionsRanker>(&["auto", "default-2024-11-15"]);
    literals::<ResponseComputerType>(&["computer"]);
    literals::<ResponseComputerUsePreviewEnvironment>(&[
        "windows", "mac", "linux", "ubuntu", "browser",
    ]);
    literals::<ResponseComputerUsePreviewType>(&["computer_use_preview"]);
    literals::<ResponseWebSearchType>(&["web_search", "web_search_2025_08_26"]);
    literals::<ResponseWebSearchSearchContextSize>(&["low", "medium", "high"]);
    literals::<ResponseWebSearchUserLocationType>(&["approximate"]);
    literals::<ResponseMcpType>(&["mcp"]);
    literals::<ResponseMcpConnectorId>(&[
        "connector_dropbox",
        "connector_gmail",
        "connector_googlecalendar",
        "connector_googledrive",
        "connector_microsoftteams",
        "connector_outlookcalendar",
        "connector_outlookemail",
        "connector_sharepoint",
    ]);
    literals::<ResponseMcpToolApprovalSetting>(&["always", "never"]);
    literals::<ResponseCodeInterpreterToolAutoType>(&["auto"]);
    literals::<ResponseCodeInterpreterToolAutoMemoryLimit>(&["1g", "4g", "16g", "64g"]);
    literals::<ResponseContainerNetworkPolicyDisabledType>(&["disabled"]);
    literals::<ResponseContainerNetworkPolicyAllowlistType>(&["allowlist"]);
    literals::<ResponseCodeInterpreterType>(&["code_interpreter"]);
    literals::<ResponseProgrammaticToolCallingType>(&["programmatic_tool_calling"]);
    literals::<ResponseImageGenerationType>(&["image_generation"]);
    literals::<ResponseImageGenerationAction>(&["generate", "edit", "auto"]);
    literals::<ResponseImageGenerationBackground>(&["transparent", "opaque", "auto"]);
    literals::<ResponseImageGenerationInputFidelity>(&["high", "low"]);
    literals::<ResponseImageGenerationModeration>(&["auto", "low"]);
    literals::<ResponseImageGenerationOutputFormat>(&["png", "webp", "jpeg"]);
    literals::<ResponseImageGenerationQuality>(&["low", "medium", "high", "xhigh", "max", "auto"]);
    literals::<ResponseLocalShellType>(&["local_shell"]);
    literals::<ResponseShellType>(&["shell"]);
    literals::<ResponseContainerAutoType>(&["container_auto"]);
    literals::<ResponseSkillReferenceType>(&["skill_reference"]);
    literals::<ResponseInlineSkillSourceMediaType>(&["application/zip"]);
    literals::<ResponseInlineSkillSourceType>(&["base64"]);
    literals::<ResponseInlineSkillType>(&["inline"]);
    literals::<ResponseLocalEnvironmentType>(&["local"]);
    literals::<ResponseContainerReferenceType>(&["container_reference"]);
    literals::<ResponseCustomType>(&["custom"]);
    literals::<ResponseTextType>(&["text"]);
    literals::<ResponseGrammarSyntax>(&["lark", "regex"]);
    literals::<ResponseGrammarType>(&["grammar"]);
    literals::<ResponseNamespaceType>(&["namespace"]);
    literals::<ResponseToolSearchType>(&["tool_search"]);
    literals::<ResponseWebSearchPreviewType>(&[
        "web_search_preview",
        "web_search_preview_2025_03_11",
    ]);
    literals::<ResponseWebSearchPreviewSearchContentTypesEntry>(&["text", "image"]);
    literals::<ResponseApplyPatchType>(&["apply_patch"]);
    literals::<ResponseToolSearchOutputType>(&["tool_search_output"]);
    literals::<ResponseAdditionalToolsRole>(&["developer"]);
    literals::<ResponseAdditionalToolsType>(&["additional_tools"]);
    literals::<ResponseConfigurationUpdateType>(&["configuration_update"]);
    literals::<ResponseReasoningEffort>(&[
        "none", "minimal", "low", "medium", "high", "xhigh", "max",
    ]);
    literals::<ResponseSummaryTextContentType>(&["summary_text"]);
    literals::<ResponseReasoningType>(&["reasoning"]);
    literals::<ResponseReasoningContentEntryType>(&["reasoning_text"]);
    literals::<ResponseCompactionType>(&["compaction"]);
    literals::<ResponseImageGenerationCallStatus>(&[
        "in_progress",
        "completed",
        "generating",
        "failed",
    ]);
    literals::<ResponseImageGenerationCallType>(&["image_generation_call"]);
    literals::<ResponseLogsType>(&["logs"]);
    literals::<ResponseImageType>(&["image"]);
    literals::<ResponseCodeInterpreterCallStatus>(&[
        "in_progress",
        "completed",
        "incomplete",
        "interpreting",
        "failed",
    ]);
    literals::<ResponseCodeInterpreterCallType>(&["code_interpreter_call"]);
    literals::<ResponseLocalShellCallActionType>(&["exec"]);
    literals::<ResponseLocalShellCallType>(&["local_shell_call"]);
    literals::<ResponseLocalShellCallOutputType>(&["local_shell_call_output"]);
    literals::<ResponseShellCallType>(&["shell_call"]);
    literals::<ResponseTimeoutType>(&["timeout"]);
    literals::<ResponseExitType>(&["exit"]);
    literals::<ResponseShellCallOutputType>(&["shell_call_output"]);
    literals::<ResponseCreateFileType>(&["create_file"]);
    literals::<ResponseDeleteFileType>(&["delete_file"]);
    literals::<ResponseUpdateFileType>(&["update_file"]);
    literals::<ResponseApplyPatchCallStatus>(&["in_progress", "completed"]);
    literals::<ResponseApplyPatchCallType>(&["apply_patch_call"]);
    literals::<ResponseApplyPatchCallOutputStatus>(&["completed", "failed"]);
    literals::<ResponseApplyPatchCallOutputType>(&["apply_patch_call_output"]);
    literals::<ResponseMcpListToolsType>(&["mcp_list_tools"]);
    literals::<ResponseMcpApprovalRequestType>(&["mcp_approval_request"]);
    literals::<ResponseMcpApprovalResponseType>(&["mcp_approval_response"]);
    literals::<ResponseMcpCallType>(&["mcp_call"]);
    literals::<ResponseMcpProtocolErrorType>(&["mcp_protocol_error"]);
    literals::<ResponseMcpToolExecutionErrorType>(&["mcp_tool_execution_error"]);
    literals::<ResponseHttpErrorType>(&["http_error"]);
    literals::<ResponseMcpCallStatus>(&[
        "in_progress",
        "completed",
        "incomplete",
        "calling",
        "failed",
    ]);
    literals::<ResponseCustomToolCallOutputType>(&["custom_tool_call_output"]);
    literals::<ResponseCustomToolCallType>(&["custom_tool_call"]);
    literals::<ResponseCompactionTriggerType>(&["compaction_trigger"]);
    literals::<ResponseItemReferenceType>(&["item_reference"]);
    literals::<ResponseProgramOutputStatus>(&["completed", "incomplete"]);
    literals::<ResponseProgramOutputType>(&["program_output"]);
}

// Fixtures below are derived from every object in the captured public item graph,
// not from serializing the implementation. Each checks full representation and
// every field's requiredness, explicit nullability, and unknown-key rejection.
#[test]
fn shared_message_and_call_object_shapes() {
    shape::<ResponseEasyInputMessage>(
        r#"{"content":"fixture","role":"user","phase":"commentary","type":"message"}"#,
        &["phase", "type"],
        &["phase"],
    );
    shape::<ResponseInputText>(
        r#"{"text":"fixture","type":"input_text","prompt_cache_breakpoint":{"mode":"explicit"}}"#,
        &["prompt_cache_breakpoint"],
        &[],
    );
    shape::<ResponseInputTextPromptCacheBreakpoint>(r#"{"mode":"explicit"}"#, &[], &[]);
    shape::<ResponseInputImage>(
        r#"{"detail":"low","type":"input_image","file_id":"fixture","image_url":"https://example.test/","prompt_cache_breakpoint":{"mode":"explicit"}}"#,
        &["file_id", "image_url", "prompt_cache_breakpoint"],
        &["file_id", "image_url"],
    );
    shape::<ResponseInputFile>(
        r#"{"type":"input_file","detail":"auto","file_data":"fixture","file_id":"fixture","file_url":"https://example.test/","filename":"fixture","prompt_cache_breakpoint":{"mode":"explicit"}}"#,
        &[
            "detail",
            "file_data",
            "file_id",
            "file_url",
            "filename",
            "prompt_cache_breakpoint",
        ],
        &["file_id"],
    );
    shape::<ResponseMessage>(
        r#"{"content":[{"text":"fixture","type":"input_text","prompt_cache_breakpoint":{"mode":"explicit"}}],"role":"user","status":"in_progress","type":"message"}"#,
        &["status", "type"],
        &[],
    );
    shape::<ResponseOutputMessage>(
        r#"{"id":"fixture","content":[{"annotations":[{"file_id":"fixture","filename":"fixture","index":1,"type":"file_citation"}],"logprobs":[{"token":"fixture","bytes":[65],"logprob":1.25,"top_logprobs":[{"token":"fixture","bytes":[65],"logprob":1.25}]}],"text":"fixture","type":"output_text"}],"role":"assistant","status":"in_progress","type":"message","phase":"commentary"}"#,
        &["phase"],
        &["phase"],
    );
    shape::<ResponseOutputText>(
        r#"{"annotations":[{"file_id":"fixture","filename":"fixture","index":1,"type":"file_citation"}],"logprobs":[{"token":"fixture","bytes":[65],"logprob":1.25,"top_logprobs":[{"token":"fixture","bytes":[65],"logprob":1.25}]}],"text":"fixture","type":"output_text"}"#,
        &[],
        &[],
    );
    shape::<ResponseFileCitation>(
        r#"{"file_id":"fixture","filename":"fixture","index":1,"type":"file_citation"}"#,
        &[],
        &[],
    );
    shape::<ResponseURLCitation>(
        r#"{"end_index":1,"start_index":1,"title":"fixture","type":"url_citation","url":"https://example.test/"}"#,
        &[],
        &[],
    );
    shape::<ResponseContainerFileCitation>(
        r#"{"container_id":"fixture","end_index":1,"file_id":"fixture","filename":"fixture","start_index":1,"type":"container_file_citation"}"#,
        &[],
        &[],
    );
    shape::<ResponseFilePath>(
        r#"{"file_id":"fixture","index":1,"type":"file_path"}"#,
        &[],
        &[],
    );
    shape::<ResponseOutputTextLogprobsEntry>(
        r#"{"token":"fixture","bytes":[65],"logprob":1.25,"top_logprobs":[{"token":"fixture","bytes":[65],"logprob":1.25}]}"#,
        &[],
        &[],
    );
    shape::<ResponseOutputTextLogprobsEntryTopLogprobsEntry>(
        r#"{"token":"fixture","bytes":[65],"logprob":1.25}"#,
        &[],
        &[],
    );
    shape::<ResponseOutputRefusal>(r#"{"refusal":"fixture","type":"refusal"}"#, &[], &[]);
    shape::<ResponseFileSearchCall>(
        r#"{"id":"fixture","queries":["fixture"],"status":"in_progress","type":"file_search_call","results":[{"attributes":{"key":"fixture"},"file_id":"fixture","filename":"fixture","score":1.25,"text":"fixture"}]}"#,
        &["results"],
        &["results"],
    );
    shape::<ResponseFileSearchCallResultsEntry>(
        r#"{"attributes":{"key":"fixture"},"file_id":"fixture","filename":"fixture","score":1.25,"text":"fixture"}"#,
        &["attributes", "file_id", "filename", "score", "text"],
        &["attributes"],
    );
    shape::<ResponseComputerCall>(
        r#"{"id":"fixture","call_id":"fixture","pending_safety_checks":[{"id":"fixture","code":"fixture","message":"fixture"}],"status":"in_progress","type":"computer_call","action":{"button":"left","type":"click","x":1,"y":1,"keys":["fixture"]},"actions":[{"button":"left","type":"click","x":1,"y":1,"keys":["fixture"]}]}"#,
        &["action", "actions"],
        &[],
    );
    shape::<ResponseComputerCallPendingSafetyChecksEntry>(
        r#"{"id":"fixture","code":"fixture","message":"fixture"}"#,
        &["code", "message"],
        &["code", "message"],
    );
    shape::<ResponseClick>(
        r#"{"button":"left","type":"click","x":1,"y":1,"keys":["fixture"]}"#,
        &["keys"],
        &["keys"],
    );
    shape::<ResponseDoubleClick>(
        r#"{"keys":["fixture"],"type":"double_click","x":1,"y":1}"#,
        &[],
        &["keys"],
    );
    shape::<ResponseDrag>(
        r#"{"path":[{"x":1,"y":1}],"type":"drag","keys":["fixture"]}"#,
        &["keys"],
        &["keys"],
    );
    shape::<ResponseDragPathEntry>(r#"{"x":1,"y":1}"#, &[], &[]);
    shape::<ResponseKeypress>(r#"{"keys":["fixture"],"type":"keypress"}"#, &[], &[]);
    shape::<ResponseMove>(
        r#"{"type":"move","x":1,"y":1,"keys":["fixture"]}"#,
        &["keys"],
        &["keys"],
    );
    shape::<ResponseScreenshot>(r#"{"type":"screenshot"}"#, &[], &[]);
    shape::<ResponseScroll>(
        r#"{"scroll_x":1,"scroll_y":1,"type":"scroll","x":1,"y":1,"keys":["fixture"]}"#,
        &["keys"],
        &["keys"],
    );
    shape::<ResponseType>(r#"{"text":"fixture","type":"type"}"#, &[], &[]);
    shape::<ResponseWait>(r#"{"type":"wait"}"#, &[], &[]);
    shape::<ResponseComputerCallOutput>(
        r#"{"call_id":"fixture","output":{"type":"computer_screenshot","file_id":"fixture","image_url":"https://example.test/"},"type":"computer_call_output","id":"fixture","acknowledged_safety_checks":[{"id":"fixture","code":"fixture","message":"fixture"}],"status":"in_progress"}"#,
        &["id", "acknowledged_safety_checks", "status"],
        &["id", "acknowledged_safety_checks", "status"],
    );
    shape::<ResponseComputerToolCallOutputScreenshot>(
        r#"{"type":"computer_screenshot","file_id":"fixture","image_url":"https://example.test/"}"#,
        &["file_id", "image_url"],
        &[],
    );
    shape::<ResponseWebSearchCall>(
        r#"{"id":"fixture","action":{"type":"search","queries":["fixture"],"query":"fixture","sources":[{"type":"url","url":"https://example.test/"}]},"status":"in_progress","type":"web_search_call"}"#,
        &[],
        &[],
    );
    shape::<ResponseSearch>(
        r#"{"type":"search","queries":["fixture"],"query":"fixture","sources":[{"type":"url","url":"https://example.test/"}]}"#,
        &["queries", "query", "sources"],
        &[],
    );
    shape::<ResponseSearchSourcesEntry>(
        r#"{"type":"url","url":"https://example.test/"}"#,
        &[],
        &[],
    );
    shape::<ResponseOpenPage>(
        r#"{"type":"open_page","url":"https://example.test/"}"#,
        &["url"],
        &["url"],
    );
    shape::<ResponseFindInPage>(
        r#"{"pattern":"fixture","type":"find_in_page","url":"https://example.test/"}"#,
        &[],
        &[],
    );
    shape::<ResponseFunctionCall>(
        r#"{"arguments":"fixture","call_id":"fixture","name":"fixture","type":"function_call","id":"fixture","async":true,"caller":{"type":"direct"},"namespace":"fixture","status":"in_progress"}"#,
        &["id", "async", "caller", "namespace", "status"],
        &["caller"],
    );
    shape::<ResponseDirect>(r#"{"type":"direct"}"#, &[], &[]);
    shape::<ResponseProgram>(r#"{"caller_id":"fixture","type":"program"}"#, &[], &[]);
    shape::<ResponseFunctionCallOutput>(
        r#"{"output":"fixture","type":"function_call_output","id":"fixture","call_id":"fixture","caller":{"type":"direct"},"name":"fixture","namespace":"fixture","status":"in_progress"}"#,
        &["id", "call_id", "caller", "name", "namespace", "status"],
        &["id", "call_id", "caller", "name", "namespace", "status"],
    );
}

#[test]
fn shared_tool_description_object_shapes() {
    shape::<ResponseInputTextContent>(
        r#"{"text":"fixture","type":"input_text","prompt_cache_breakpoint":{"mode":"explicit"}}"#,
        &["prompt_cache_breakpoint"],
        &["prompt_cache_breakpoint"],
    );
    shape::<ResponseInputImageContent>(
        r#"{"type":"input_image","detail":"low","file_id":"fixture","image_url":"https://example.test/","prompt_cache_breakpoint":{"mode":"explicit"}}"#,
        &["detail", "file_id", "image_url", "prompt_cache_breakpoint"],
        &["detail", "file_id", "image_url", "prompt_cache_breakpoint"],
    );
    shape::<ResponseInputFileContent>(
        r#"{"type":"input_file","detail":"auto","file_data":"fixture","file_id":"fixture","file_url":"https://example.test/","filename":"fixture","prompt_cache_breakpoint":{"mode":"explicit"}}"#,
        &[
            "detail",
            "file_data",
            "file_id",
            "file_url",
            "filename",
            "prompt_cache_breakpoint",
        ],
        &[
            "file_data",
            "file_id",
            "file_url",
            "filename",
            "prompt_cache_breakpoint",
        ],
    );
    shape::<ResponseProgramCaller>(r#"{"caller_id":"fixture","type":"program"}"#, &[], &[]);
    shape::<ResponseToolSearchCall>(
        r#"{"arguments":{"arbitrary":["fixture",null,1]},"type":"tool_search_call","id":"fixture","call_id":"fixture","execution":"server","status":"in_progress"}"#,
        &["id", "call_id", "execution", "status"],
        &["arguments", "id", "call_id", "status"],
    );
    shape::<ResponseToolSearchOutput>(
        r#"{"tools":[{"name":"fixture","parameters":{"key":{"arbitrary":["fixture",null,1]}},"strict":true,"type":"function","allowed_callers":["direct"],"async":true,"defer_loading":true,"description":"fixture","output_schema":{"key":{"arbitrary":["fixture",null,1]}}}],"type":"tool_search_output","id":"fixture","call_id":"fixture","execution":"server","status":"in_progress"}"#,
        &["id", "call_id", "execution", "status"],
        &["id", "call_id", "status"],
    );
    shape::<ResponseFunction>(
        r#"{"name":"fixture","parameters":{"key":{"arbitrary":["fixture",null,1]}},"strict":true,"type":"function","allowed_callers":["direct"],"async":true,"defer_loading":true,"description":"fixture","output_schema":{"key":{"arbitrary":["fixture",null,1]}}}"#,
        &[
            "allowed_callers",
            "async",
            "defer_loading",
            "description",
            "output_schema",
        ],
        &[
            "parameters",
            "strict",
            "allowed_callers",
            "description",
            "output_schema",
        ],
    );
    shape::<ResponseFileSearch>(
        r#"{"type":"file_search","vector_store_ids":["fixture"],"filters":{"key":"fixture","type":"eq","value":"fixture"},"max_num_results":1,"ranking_options":{"hybrid_search":{"embedding_weight":1.25,"text_weight":1.25},"ranker":"auto","score_threshold":1.25}}"#,
        &["filters", "max_num_results", "ranking_options"],
        &["filters"],
    );
    shape::<ResponseComparisonFilter>(
        r#"{"key":"fixture","type":"eq","value":"fixture"}"#,
        &[],
        &[],
    );
    shape::<ResponseCompoundFilter>(
        r#"{"filters":[{"key":"fixture","type":"eq","value":"fixture"}],"type":"and"}"#,
        &[],
        &[],
    );
    shape::<ResponseFileSearchRankingOptions>(
        r#"{"hybrid_search":{"embedding_weight":1.25,"text_weight":1.25},"ranker":"auto","score_threshold":1.25}"#,
        &["hybrid_search", "ranker", "score_threshold"],
        &[],
    );
    shape::<ResponseFileSearchRankingOptionsHybridSearch>(
        r#"{"embedding_weight":1.25,"text_weight":1.25}"#,
        &[],
        &[],
    );
    shape::<ResponseComputer>(r#"{"type":"computer"}"#, &[], &[]);
    shape::<ResponseComputerUsePreview>(
        r#"{"display_height":1,"display_width":1,"environment":"windows","type":"computer_use_preview"}"#,
        &[],
        &[],
    );
    shape::<ResponseWebSearch>(
        r#"{"type":"web_search","external_web_access":true,"filters":{"allowed_domains":["fixture"]},"search_context_size":"low","user_location":{"city":"fixture","country":"fixture","region":"fixture","timezone":"fixture","type":"approximate"}}"#,
        &[
            "external_web_access",
            "filters",
            "search_context_size",
            "user_location",
        ],
        &["filters", "user_location"],
    );
    shape::<ResponseWebSearchFilters>(
        r#"{"allowed_domains":["fixture"]}"#,
        &["allowed_domains"],
        &["allowed_domains"],
    );
    shape::<ResponseWebSearchUserLocation>(
        r#"{"city":"fixture","country":"fixture","region":"fixture","timezone":"fixture","type":"approximate"}"#,
        &["city", "country", "region", "timezone", "type"],
        &["city", "country", "region", "timezone"],
    );
    shape::<ResponseMcp>(
        r#"{"server_label":"fixture","type":"mcp","allowed_callers":["direct"],"allowed_tools":["fixture"],"authorization":"fixture","connector_id":"connector_dropbox","defer_loading":true,"headers":{"key":"fixture"},"require_approval":{"always":{"read_only":true,"tool_names":["fixture"]},"never":{"read_only":true,"tool_names":["fixture"]}},"server_description":"fixture","server_url":"https://example.test/","tunnel_id":"fixture"}"#,
        &[
            "allowed_callers",
            "allowed_tools",
            "authorization",
            "connector_id",
            "defer_loading",
            "headers",
            "require_approval",
            "server_description",
            "server_url",
            "tunnel_id",
        ],
        &[
            "allowed_callers",
            "allowed_tools",
            "headers",
            "require_approval",
        ],
    );
    shape::<ResponseMcpToolFilter>(
        r#"{"read_only":true,"tool_names":["fixture"]}"#,
        &["read_only", "tool_names"],
        &[],
    );
    shape::<ResponseMcpToolApprovalFilter>(
        r#"{"always":{"read_only":true,"tool_names":["fixture"]},"never":{"read_only":true,"tool_names":["fixture"]}}"#,
        &["always", "never"],
        &[],
    );
    shape::<ResponseCodeInterpreter>(
        r#"{"container":"fixture","type":"code_interpreter","allowed_callers":["direct"]}"#,
        &["allowed_callers"],
        &["allowed_callers"],
    );
    shape::<ResponseCodeInterpreterToolAuto>(
        r#"{"type":"auto","file_ids":["fixture"],"memory_limit":"1g","network_policy":{"type":"disabled"}}"#,
        &["file_ids", "memory_limit", "network_policy"],
        &["memory_limit"],
    );
    shape::<ResponseContainerNetworkPolicyDisabled>(r#"{"type":"disabled"}"#, &[], &[]);
    shape::<ResponseContainerNetworkPolicyAllowlist>(
        r#"{"allowed_domains":["fixture"],"type":"allowlist","domain_secrets":[{"domain":"fixture","name":"fixture","value":"fixture"}]}"#,
        &["domain_secrets"],
        &[],
    );
    shape::<ResponseContainerNetworkPolicyDomainSecret>(
        r#"{"domain":"fixture","name":"fixture","value":"fixture"}"#,
        &[],
        &[],
    );
    shape::<ResponseProgrammaticToolCalling>(r#"{"type":"programmatic_tool_calling"}"#, &[], &[]);
    shape::<ResponseImageGeneration>(
        r#"{"type":"image_generation","action":"generate","background":"transparent","input_fidelity":"high","input_image_mask":{"file_id":"fixture","image_url":"fixture"},"model":"fixture","moderation":"auto","output_compression":1,"output_format":"png","partial_images":1,"quality":"low","size":"fixture"}"#,
        &[
            "action",
            "background",
            "input_fidelity",
            "input_image_mask",
            "model",
            "moderation",
            "output_compression",
            "output_format",
            "partial_images",
            "quality",
            "size",
        ],
        &["input_fidelity"],
    );
    shape::<ResponseImageGenerationInputImageMask>(
        r#"{"file_id":"fixture","image_url":"fixture"}"#,
        &["file_id", "image_url"],
        &[],
    );
    shape::<ResponseLocalShell>(r#"{"type":"local_shell"}"#, &[], &[]);
    shape::<ResponseShell>(
        r#"{"type":"shell","allowed_callers":["direct"],"environment":{"type":"container_auto","file_ids":["fixture"],"memory_limit":"1g","network_policy":{"type":"disabled"},"skills":[{"skill_id":"fixture","type":"skill_reference","version":"fixture"}]}}"#,
        &["allowed_callers", "environment"],
        &["allowed_callers", "environment"],
    );
    shape::<ResponseContainerAuto>(
        r#"{"type":"container_auto","file_ids":["fixture"],"memory_limit":"1g","network_policy":{"type":"disabled"},"skills":[{"skill_id":"fixture","type":"skill_reference","version":"fixture"}]}"#,
        &["file_ids", "memory_limit", "network_policy", "skills"],
        &["memory_limit"],
    );
    shape::<ResponseSkillReference>(
        r#"{"skill_id":"fixture","type":"skill_reference","version":"fixture"}"#,
        &["version"],
        &[],
    );
    shape::<ResponseInlineSkill>(
        r#"{"description":"fixture","name":"fixture","source":{"data":"fixture","media_type":"application/zip","type":"base64"},"type":"inline"}"#,
        &[],
        &[],
    );
    shape::<ResponseInlineSkillSource>(
        r#"{"data":"fixture","media_type":"application/zip","type":"base64"}"#,
        &[],
        &[],
    );
    shape::<ResponseLocalEnvironment>(
        r#"{"type":"local","skills":[{"description":"fixture","name":"fixture","path":"fixture"}]}"#,
        &["skills"],
        &[],
    );
    shape::<ResponseLocalSkill>(
        r#"{"description":"fixture","name":"fixture","path":"fixture"}"#,
        &[],
        &[],
    );
    shape::<ResponseContainerReference>(
        r#"{"container_id":"fixture","type":"container_reference"}"#,
        &[],
        &[],
    );
    shape::<ResponseCustom>(
        r#"{"name":"fixture","type":"custom","allowed_callers":["direct"],"async":true,"defer_loading":true,"description":"fixture","format":{"type":"text"}}"#,
        &[
            "allowed_callers",
            "async",
            "defer_loading",
            "description",
            "format",
        ],
        &["allowed_callers"],
    );
    shape::<ResponseText>(r#"{"type":"text"}"#, &[], &[]);
    shape::<ResponseGrammar>(
        r#"{"definition":"fixture","syntax":"lark","type":"grammar"}"#,
        &[],
        &[],
    );
    shape::<ResponseNamespace>(
        r#"{"description":"fixture","name":"fixture","tools":[{"name":"fixture","type":"function","allowed_callers":["direct"],"async":true,"defer_loading":true,"description":"fixture","output_schema":{"key":{"arbitrary":["fixture",null,1]}},"parameters":{"arbitrary":["fixture",null,1]},"strict":true}],"type":"namespace"}"#,
        &[],
        &[],
    );
    shape::<ResponseNamespaceFunction>(
        r#"{"name":"fixture","type":"function","allowed_callers":["direct"],"async":true,"defer_loading":true,"description":"fixture","output_schema":{"key":{"arbitrary":["fixture",null,1]}},"parameters":{"arbitrary":["fixture",null,1]},"strict":true}"#,
        &[
            "allowed_callers",
            "async",
            "defer_loading",
            "description",
            "output_schema",
            "parameters",
            "strict",
        ],
        &[
            "allowed_callers",
            "description",
            "output_schema",
            "parameters",
            "strict",
        ],
    );
    shape::<ResponseToolSearch>(
        r#"{"type":"tool_search","description":"fixture","execution":"server","parameters":{"arbitrary":["fixture",null,1]}}"#,
        &["description", "execution", "parameters"],
        &["description", "parameters"],
    );
    shape::<ResponseWebSearchPreview>(
        r#"{"type":"web_search_preview","search_content_types":["text"],"search_context_size":"low","user_location":{"type":"approximate","city":"fixture","country":"fixture","region":"fixture","timezone":"fixture"}}"#,
        &[
            "search_content_types",
            "search_context_size",
            "user_location",
        ],
        &["user_location"],
    );
    shape::<ResponseWebSearchPreviewUserLocation>(
        r#"{"type":"approximate","city":"fixture","country":"fixture","region":"fixture","timezone":"fixture"}"#,
        &["city", "country", "region", "timezone"],
        &["city", "country", "region", "timezone"],
    );
}

#[test]
fn remaining_shared_item_object_shapes() {
    shape::<ResponseApplyPatch>(
        r#"{"type":"apply_patch","allowed_callers":["direct"]}"#,
        &["allowed_callers"],
        &["allowed_callers"],
    );
    shape::<ResponseAdditionalTools>(
        r#"{"role":"developer","tools":[{"name":"fixture","parameters":{"key":{"arbitrary":["fixture",null,1]}},"strict":true,"type":"function","allowed_callers":["direct"],"async":true,"defer_loading":true,"description":"fixture","output_schema":{"key":{"arbitrary":["fixture",null,1]}}}],"type":"additional_tools","id":"fixture"}"#,
        &["id"],
        &["id"],
    );
    shape::<ResponseConfigurationUpdate>(
        r#"{"type":"configuration_update","id":"fixture","reasoning":{"effort":"none"}}"#,
        &["id", "reasoning"],
        &["id"],
    );
    shape::<ResponseConfigurationUpdateReasoning>(r#"{"effort":"none"}"#, &["effort"], &["effort"]);
    shape::<ResponseReasoning>(
        r#"{"id":"fixture","summary":[{"text":"fixture","type":"summary_text"}],"type":"reasoning","content":[{"text":"fixture","type":"reasoning_text"}],"encrypted_content":"fixture","status":"in_progress"}"#,
        &["content", "encrypted_content", "status"],
        &["encrypted_content"],
    );
    shape::<ResponseSummaryTextContent>(r#"{"text":"fixture","type":"summary_text"}"#, &[], &[]);
    shape::<ResponseReasoningContentEntry>(
        r#"{"text":"fixture","type":"reasoning_text"}"#,
        &[],
        &[],
    );
    shape::<ResponseCompaction>(
        r#"{"encrypted_content":"fixture","type":"compaction","id":"fixture"}"#,
        &["id"],
        &["id"],
    );
    shape::<ResponseImageGenerationCall>(
        r#"{"id":"fixture","result":"fixture","status":"in_progress","type":"image_generation_call","action":"generate","background":"transparent","output_format":"png","quality":"low","revised_prompt":"fixture","size":"fixture"}"#,
        &[
            "action",
            "background",
            "output_format",
            "quality",
            "revised_prompt",
            "size",
        ],
        &[
            "result",
            "action",
            "background",
            "output_format",
            "quality",
            "revised_prompt",
            "size",
        ],
    );
    shape::<ResponseCodeInterpreterCall>(
        r#"{"id":"fixture","code":"fixture","container_id":"fixture","outputs":[{"logs":"fixture","type":"logs"}],"status":"in_progress","type":"code_interpreter_call"}"#,
        &[],
        &["code", "outputs"],
    );
    shape::<ResponseLogs>(r#"{"logs":"fixture","type":"logs"}"#, &[], &[]);
    shape::<ResponseImage>(
        r#"{"type":"image","url":"https://example.test/"}"#,
        &[],
        &[],
    );
    shape::<ResponseLocalShellCall>(
        r#"{"id":"fixture","action":{"command":["fixture"],"env":{"key":"fixture"},"type":"exec","timeout_ms":1,"user":"fixture","working_directory":"fixture"},"call_id":"fixture","status":"in_progress","type":"local_shell_call"}"#,
        &[],
        &[],
    );
    shape::<ResponseLocalShellCallAction>(
        r#"{"command":["fixture"],"env":{"key":"fixture"},"type":"exec","timeout_ms":1,"user":"fixture","working_directory":"fixture"}"#,
        &["timeout_ms", "user", "working_directory"],
        &["timeout_ms", "user", "working_directory"],
    );
    shape::<ResponseLocalShellCallOutput>(
        r#"{"id":"fixture","output":"fixture","type":"local_shell_call_output","status":"in_progress"}"#,
        &["status"],
        &["status"],
    );
    shape::<ResponseShellCall>(
        r#"{"action":{"commands":["fixture"],"max_output_length":1,"timeout_ms":1},"call_id":"fixture","type":"shell_call","id":"fixture","caller":{"type":"direct"},"environment":{"type":"local","skills":[{"description":"fixture","name":"fixture","path":"fixture"}]},"status":"in_progress"}"#,
        &["id", "caller", "environment", "status"],
        &["id", "caller", "environment", "status"],
    );
    shape::<ResponseShellCallAction>(
        r#"{"commands":["fixture"],"max_output_length":1,"timeout_ms":1}"#,
        &["max_output_length", "timeout_ms"],
        &["max_output_length", "timeout_ms"],
    );
    shape::<ResponseShellCallOutput>(
        r#"{"call_id":"fixture","output":[{"outcome":{"type":"timeout"},"stderr":"fixture","stdout":"fixture"}],"type":"shell_call_output","id":"fixture","caller":{"type":"direct"},"max_output_length":1,"status":"in_progress"}"#,
        &["id", "caller", "max_output_length", "status"],
        &["id", "caller", "max_output_length", "status"],
    );
    shape::<ResponseFunctionShellCallOutputContent>(
        r#"{"outcome":{"type":"timeout"},"stderr":"fixture","stdout":"fixture"}"#,
        &[],
        &[],
    );
    shape::<ResponseTimeout>(r#"{"type":"timeout"}"#, &[], &[]);
    shape::<ResponseExit>(r#"{"exit_code":1,"type":"exit"}"#, &[], &[]);
    shape::<ResponseApplyPatchCall>(
        r#"{"call_id":"fixture","operation":{"diff":"fixture","path":"fixture","type":"create_file"},"status":"in_progress","type":"apply_patch_call","id":"fixture","caller":{"type":"direct"}}"#,
        &["id", "caller"],
        &["id", "caller"],
    );
    shape::<ResponseCreateFile>(
        r#"{"diff":"fixture","path":"fixture","type":"create_file"}"#,
        &[],
        &[],
    );
    shape::<ResponseDeleteFile>(r#"{"path":"fixture","type":"delete_file"}"#, &[], &[]);
    shape::<ResponseUpdateFile>(
        r#"{"diff":"fixture","path":"fixture","type":"update_file"}"#,
        &[],
        &[],
    );
    shape::<ResponseApplyPatchCallOutput>(
        r#"{"call_id":"fixture","status":"completed","type":"apply_patch_call_output","id":"fixture","caller":{"type":"direct"},"output":"fixture"}"#,
        &["id", "caller", "output"],
        &["id", "caller", "output"],
    );
    shape::<ResponseMcpListTools>(
        r#"{"id":"fixture","server_label":"fixture","tools":[{"input_schema":{"arbitrary":["fixture",null,1]},"name":"fixture","annotations":{"arbitrary":["fixture",null,1]},"description":"fixture"}],"type":"mcp_list_tools","error":"fixture"}"#,
        &["error"],
        &["error"],
    );
    shape::<ResponseMcpListToolsToolsEntry>(
        r#"{"input_schema":{"arbitrary":["fixture",null,1]},"name":"fixture","annotations":{"arbitrary":["fixture",null,1]},"description":"fixture"}"#,
        &["annotations", "description"],
        &["input_schema", "annotations", "description"],
    );
    shape::<ResponseMcpApprovalRequest>(
        r#"{"id":"fixture","arguments":"fixture","name":"fixture","server_label":"fixture","type":"mcp_approval_request"}"#,
        &[],
        &[],
    );
    shape::<ResponseMcpApprovalResponse>(
        r#"{"approval_request_id":"fixture","approve":true,"type":"mcp_approval_response","id":"fixture","reason":"fixture"}"#,
        &["id", "reason"],
        &["id", "reason"],
    );
    shape::<ResponseMcpCall>(
        r#"{"id":"fixture","arguments":"fixture","name":"fixture","server_label":"fixture","type":"mcp_call","approval_request_id":"fixture","error":{"code":1,"message":"fixture","type":"mcp_protocol_error"},"output":"fixture","status":"in_progress"}"#,
        &["approval_request_id", "error", "output", "status"],
        &["approval_request_id", "error", "output"],
    );
    shape::<ResponseMcpProtocolError>(
        r#"{"code":1,"message":"fixture","type":"mcp_protocol_error"}"#,
        &[],
        &[],
    );
    shape::<ResponseMcpToolExecutionError>(
        r#"{"content":{"arbitrary":["fixture",null,1]},"type":"mcp_tool_execution_error"}"#,
        &[],
        &["content"],
    );
    shape::<ResponseHttpError>(
        r#"{"code":1,"message":"fixture","type":"http_error"}"#,
        &[],
        &[],
    );
    shape::<ResponseCustomToolCallOutput>(
        r#"{"call_id":"fixture","output":"fixture","type":"custom_tool_call_output","id":"fixture","caller":{"type":"direct"}}"#,
        &["id", "caller"],
        &["caller"],
    );
    shape::<ResponseCustomToolCall>(
        r#"{"call_id":"fixture","input":"fixture","name":"fixture","type":"custom_tool_call","id":"fixture","async":true,"caller":{"type":"direct"},"namespace":"fixture"}"#,
        &["id", "async", "caller", "namespace"],
        &["caller"],
    );
    shape::<ResponseCompactionTrigger>(
        r#"{"type":"compaction_trigger","id":"fixture"}"#,
        &["id"],
        &["id"],
    );
    shape::<ResponseItemReference>(
        r#"{"id":"fixture","type":"item_reference"}"#,
        &["type"],
        &["type"],
    );
    shape::<ResponseProgramItem>(
        r#"{"id":"fixture","call_id":"fixture","code":"fixture","fingerprint":"fixture","type":"program"}"#,
        &[],
        &[],
    );
    shape::<ResponseProgramOutput>(
        r#"{"id":"fixture","call_id":"fixture","result":"fixture","status":"completed","type":"program_output"}"#,
        &[],
        &[],
    );
}

#[test]
fn ergonomic_constructors_and_sparse_nullable_output() {
    let text = ResponseInputItem::user_text("user-secret");
    assert_eq!(
        serde_json::to_value(&text).unwrap(),
        json!({"type":"message","role":"user","content":"user-secret"})
    );
    let image =
        ResponseInputItem::user_image("data:image/png;base64,AAAA", ResponseImageDetail::Auto);
    assert_eq!(
        serde_json::to_value(&image).unwrap(),
        json!({"type":"message","role":"user","content":[{"type":"input_image","detail":"auto","image_url":"data:image/png;base64,AAAA"}]})
    );
    let output = ResponseInputItem::function_call_output("call_1", r#"{"ok":true}"#);
    assert_eq!(
        serde_json::to_value(&output).unwrap(),
        json!({"type":"function_call_output","call_id":"call_1","output":"{\"ok\":true}"})
    );
    for item in [text, image, output] {
        item.validate().unwrap();
        assert_eq!(
            serde_json::from_value::<ResponseInputItem>(serde_json::to_value(&item).unwrap())
                .unwrap(),
            item
        );
    }
    for wire in [
        json!({"type":"function_call_output","output":""}),
        json!({"type":"function_call_output","output":"","call_id":null}),
        json!({"type":"function_call_output","output":"","call_id":"call"}),
        json!({"id":"previous"}),
        json!({"id":"previous","type":null}),
    ] {
        let item: ResponseInputItem = serde_json::from_value(wire.clone()).unwrap();
        item.validate().unwrap();
        assert_eq!(serde_json::to_value(item).unwrap(), wire);
    }
}

#[test]
fn faithful_recursive_validation_and_integer_types() {
    assert!(ResponseInputItem::user_text("").validate().is_ok());
    assert!(
        ResponseInputItem::function_call_output("", "")
            .validate()
            .is_err()
    );
    assert!(
        ResponseInputItem::function_call_output("é".repeat(64), "")
            .validate()
            .is_ok()
    );
    assert!(
        ResponseInputItem::function_call_output("é".repeat(65), "")
            .validate()
            .is_err()
    );
    for value in [
        json!({"type":"apply_patch_call","call_id":"call","status":"completed","operation":{"type":"delete_file","path":""}}),
        json!({"type":"function_call_output","output":"","caller":{"type":"program","caller_id":""}}),
        json!({"type":"function_call_output","output":[{"type":"input_image","image_url":"not a URI"}]}),
        json!({"type":"additional_tools","role":"developer","tools":[{"type":"image_generation","output_compression":101}]}),
        json!({"type":"tool_search_output","tools":[{"type":"image_generation","partial_images":-1}]}),
        json!({"type":"tool_search_output","tools":[{"type":"namespace","name":"","description":"","tools":[]}]}),
    ] {
        let item: ResponseInputItem = serde_json::from_value(value).unwrap();
        assert!(item.validate().is_err());
    }
    for value in [
        json!({"type":"tool_search_output","tools":[{"type":"image_generation","output_compression":50.5}]}),
        json!({"type":"computer_call","id":"i","call_id":"c","pending_safety_checks":[],"status":"completed","action":{"type":"click","button":"left","x":1.5,"y":0}}),
        json!({"type":"shell_call","call_id":"c","action":{"commands":[],"timeout_ms":"100"}}),
    ] {
        assert!(serde_json::from_value::<ResponseInputItem>(value).is_err());
    }
    let mut item: ResponseInputItem = serde_json::from_value(json!({
        "type":"file_search_call","id":"i","queries":[],"status":"completed","results":[{"score":0.5}]
    })).unwrap();
    let ResponseInputItem::FileSearchCall(call) = &mut item else {
        panic!("file search variant")
    };
    let Field::Value(results) = &mut call.results else {
        panic!("results present")
    };
    results[0].score = Some(f64::NAN);
    assert!(item.validate().is_err());
    let oversized = ResponseInputItem::Program(ResponseProgramItem {
        id: "id".into(),
        call_id: "call".into(),
        code: "x".repeat(10_485_761),
        fingerprint: String::new(),
        r#type: ResponseProgramType::Program,
    });
    assert!(oversized.validate().is_err());
}

#[test]
fn malformed_known_input_never_matches_another_variant() {
    for value in [
        json!({"type":"future_item","id":"item"}),
        json!({"type":"function_call","id":"item","call_id":"call","arguments":"{}"}),
        json!({"type":"function_call_output","id":"item"}),
        json!({"type":"message","id":"item","role":"assistant","status":"completed","content":[{"type":"output_text","text":"x","annotations":[]}]}),
        json!({"type":"function_call_output","output":[{"type":"input_text","text":"x","unknown":true}]}),
        json!({"type":"function_call_output","output":[],"metadata":{}}),
        Value::Null,
        json!([]),
        json!("not an item"),
    ] {
        assert!(serde_json::from_value::<ResponseInputItem>(value).is_err());
    }
    // Record keys remain open, while record value types remain constrained.
    roundtrip::<ResponseInputItem>(
        r#"{"type":"tool_search_call","arguments":{"any":["shape",null,42]}}"#,
    );
    assert!(
        serde_json::from_value::<ResponseInputItem>(json!({
            "type":"local_shell_call","id":"i","call_id":"c","status":"completed",
            "action":{"type":"exec","command":[],"env":{"KEY":false}}
        }))
        .is_err()
    );
}

fn lifecycle(event_type: &str, id: &str) -> Value {
    json!({
        "type":event_type,"sequence_number":0,
        "response":{"id":id,"created_at":1_700_000_000.25,"completed_at":null,
            "status":"in_progress","output":[],"tools":[],"instructions":null}
    })
}

fn finished_call(call_id: &str) -> Value {
    json!({
        "type":"response.output_item.done","sequence_number":2,"output_index":0,
        "item":{"type":"function_call","id":"item_1","call_id":call_id,
            "name":"lookup","arguments":"{\"city\":\"Paris\"}","status":"completed"}
    })
}

#[test]
fn nested_function_calls_are_actionable_only_on_output_item_done() {
    let done = ResponseEvent::decode(finished_call("call_1")).unwrap();
    let call = done.completed_function_call().unwrap();
    assert_eq!(call.name, "lookup");
    assert_eq!(call.args, r#"{"city":"Paris"}"#);
    assert_eq!(call.call_id, "call_1");
    assert_eq!(call.id.as_deref(), Some("item_1"));
    let mut added = finished_call("call_1");
    added["type"] = json!("response.output_item.added");
    let mut snapshot = lifecycle("response.completed", "response_1");
    snapshot["response"]["output"] = json!([finished_call("call_1")["item"].clone()]);
    let arguments_done = json!({
        "type":"response.function_call_arguments.done","sequence_number":1,
        "output_index":0,"item_id":"item_1","arguments":"{\"city\":\"Paris\"}"
    });
    let delta = json!({
        "type":"response.function_call_arguments.delta","sequence_number":1,
        "output_index":0,"item_id":"item_1","delta":"{\"city\":"
    });
    for value in [added, snapshot, arguments_done, delta] {
        assert!(
            ResponseEvent::decode(value)
                .unwrap()
                .completed_function_call()
                .is_none()
        );
    }
    for status in ["incomplete", "in_progress"] {
        let mut incomplete = finished_call("call_1");
        incomplete["item"]["status"] = json!(status);
        assert!(
            ResponseEvent::decode(incomplete)
                .unwrap()
                .completed_function_call()
                .is_none()
        );
    }
    let mut minimal = finished_call("call_1");
    minimal["item"].as_object_mut().unwrap().remove("id");
    minimal["item"].as_object_mut().unwrap().remove("status");
    assert!(
        ResponseEvent::decode(minimal)
            .unwrap()
            .completed_function_call()
            .is_some()
    );
}

#[test]
fn nested_known_event_fields_are_required_and_future_fields_are_ignored() {
    let mut fixtures: Vec<Value> = [
        "response.created",
        "response.in_progress",
        "response.completed",
        "response.failed",
        "response.incomplete",
        "response.queued",
    ]
    .iter()
    .map(|name| lifecycle(name, "response_1"))
    .collect();
    fixtures.push(finished_call("call_1"));
    let mut added = finished_call("call_1");
    added["type"] = json!("response.output_item.added");
    fixtures.push(added);
    fixtures.extend([
        json!({"type":"response.function_call_arguments.delta","sequence_number":1,"output_index":0,"item_id":"i","delta":"{"}),
        json!({"type":"response.function_call_arguments.done","sequence_number":1,"output_index":0,"item_id":"i","arguments":"{}"}),
        json!({"type":"response.output_text.delta","sequence_number":1,"output_index":0,"content_index":0,"item_id":"i","delta":"hi","logprobs":[]}),
        json!({"type":"response.output_text.done","sequence_number":1,"output_index":0,"content_index":0,"item_id":"i","text":"hi","logprobs":[]}),
    ]);
    for fixture in fixtures {
        let parsed = ResponseEvent::decode(fixture.clone()).unwrap();
        assert!(!matches!(parsed, ResponseEvent::Unknown { .. }));
        assert_eq!(
            serde_json::from_value::<ResponseEvent>(fixture.clone()).unwrap(),
            parsed
        );
        let mut extended = fixture.clone();
        extended["future"] = json!({"secret":"ignored"});
        assert_eq!(ResponseEvent::decode(extended).unwrap(), parsed);
        for key in fixture.as_object().unwrap().keys() {
            let mut missing = fixture.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(ResponseEvent::decode(missing).is_err(), "missing {key}");
            let mut null = fixture.clone();
            null[key] = Value::Null;
            assert!(ResponseEvent::decode(null).is_err(), "null {key}");
        }
    }
}

#[test]
fn malformed_known_nested_items_and_timestamps_are_errors() {
    for key in ["type", "name", "arguments", "call_id"] {
        let mut missing = finished_call("call");
        missing["item"].as_object_mut().unwrap().remove(key);
        assert!(ResponseEvent::decode(missing).is_err(), "missing {key}");
        let mut null = finished_call("call");
        null["item"][key] = Value::Null;
        assert!(ResponseEvent::decode(null).is_err(), "null {key}");
    }
    let mut null_id = finished_call("call");
    null_id["item"]["id"] = Value::Null;
    assert!(ResponseEvent::decode(null_id).is_err());
    let mut invalid_args = finished_call("call");
    invalid_args["item"]["arguments"] = json!({"not":"a string"});
    assert!(ResponseEvent::decode(invalid_args).is_err());
    for key in ["created_at", "completed_at"] {
        let mut invalid = lifecycle("response.created", "r");
        invalid["response"][key] = json!("1700000000.25");
        assert!(ResponseEvent::decode(invalid).is_err());
    }
    for key in ["id", "created_at", "output"] {
        let mut invalid = lifecycle("response.completed", "r");
        invalid["response"].as_object_mut().unwrap().remove(key);
        assert!(ResponseEvent::decode(invalid).is_err());
    }
}

#[test]
fn future_events_and_items_retain_raw_maps_without_becoming_calls() {
    let value = json!({"type":"response.future.event","nested":{"secret":[1,null,"payload"]}});
    let ResponseEvent::Unknown { event_type, raw } = ResponseEvent::decode(value.clone()).unwrap()
    else {
        panic!("unknown event must be retained");
    };
    assert_eq!(event_type, "response.future.event");
    assert_eq!(Value::Object(raw), value);
    let mut unknown_item = finished_call("call");
    unknown_item["item"] = json!({"type":"future_item","payload":"secret"});
    let event = ResponseEvent::decode(unknown_item.clone()).unwrap();
    assert!(event.completed_function_call().is_none());
    let ResponseEvent::OutputItemDone {
        item: ResponseEventItem::Other { raw, .. },
        ..
    } = event
    else {
        panic!("future output item retained");
    };
    assert_eq!(Value::Object(raw), unknown_item["item"]);
    for invalid in [
        json!({}),
        Value::Null,
        json!([]),
        json!({"type":null}),
        json!({"type":42}),
    ] {
        assert!(ResponseEvent::decode(invalid).is_err());
    }
}

#[test]
fn per_response_calls_survive_empty_terminal_output_and_multiple_calls() {
    let mut tracker = FunctionCallTracker::default();
    let first = ResponseEvent::decode(finished_call("call_1")).unwrap();
    assert!(tracker.observe(&first).is_err());
    tracker
        .observe(&ResponseEvent::decode(lifecycle("response.created", "r1")).unwrap())
        .unwrap();
    tracker.observe(&first).unwrap();
    tracker
        .observe(&ResponseEvent::decode(finished_call("call_2")).unwrap())
        .unwrap();
    tracker.observe(&first).unwrap();
    for kind in ["response.in_progress", "response.completed"] {
        let event = ResponseEvent::decode(lifecycle(kind, "r1")).unwrap();
        tracker.observe(&event).unwrap();
        assert_eq!(tracker.calls("r1").unwrap().len(), 2);
    }
    tracker
        .observe(&ResponseEvent::decode(lifecycle("response.created", "r2")).unwrap())
        .unwrap();
    tracker
        .observe(&ResponseEvent::decode(finished_call("call_3")).unwrap())
        .unwrap();
    assert_eq!(tracker.active_response_id(), Some("r2"));
    assert_eq!(tracker.calls("r1").unwrap().len(), 2);
    assert_eq!(tracker.calls("r2").unwrap()[0].call_id, "call_3");
    let mut conflicting = finished_call("call_3");
    conflicting["item"]["arguments"] = json!("different");
    assert!(
        tracker
            .observe(&ResponseEvent::decode(conflicting).unwrap())
            .is_err()
    );
    assert_eq!(tracker.calls("r2").unwrap().len(), 1);
    assert_eq!(tracker.remove("r1").unwrap().len(), 2);
    assert_eq!(tracker.active_response_id(), Some("r2"));
    assert_eq!(tracker.remove("r2").unwrap().len(), 1);
    assert_eq!(tracker.active_response_id(), None);
}

#[test]
fn debug_redacts_inputs_functions_unknown_events_and_tracking() {
    let secret = "do-not-log-this-payload";
    let input = ResponseInputItem::user_text(secret);
    assert!(!format!("{input:?}").contains(secret));
    let output = ResponseInputItem::function_call_output(secret, secret);
    assert!(!format!("{output:?}").contains(secret));
    let mut value = finished_call(secret);
    value["item"]["arguments"] = json!(secret);
    value["item"]["name"] = json!(secret);
    let event = ResponseEvent::decode(value).unwrap();
    assert!(!format!("{event:?}").contains(secret));
    assert!(!format!("{:?}", event.completed_function_call().unwrap()).contains(secret));
    let mut tracker = FunctionCallTracker::default();
    tracker
        .observe(&ResponseEvent::decode(lifecycle("response.created", secret)).unwrap())
        .unwrap();
    tracker.observe(&event).unwrap();
    assert!(!format!("{tracker:?}").contains(secret));
    let unknown = ResponseEvent::decode(json!({"type":secret,"content":secret})).unwrap();
    assert!(!format!("{unknown:?}").contains(secret));
    let snapshot = ResponseEvent::decode(lifecycle("response.created", secret)).unwrap();
    assert!(!format!("{snapshot:?}").contains(secret));
}

#[test]
fn lifecycle_timestamps_and_stream_logprobs_use_strict_numbers() {
    let mut wire = lifecycle("response.completed", "r");
    wire["response"]["completed_at"] = json!(1_700_000_001.75);
    let event = ResponseEvent::decode(wire.clone()).unwrap();
    let ResponseEvent::Lifecycle { response, .. } = event else {
        panic!("lifecycle projection");
    };
    assert_eq!(response.created_at, 1_700_000_000.25);
    assert_eq!(response.completed_at, Field::Value(1_700_000_001.75));
    assert!(response.output.is_empty());
    wire["response"]
        .as_object_mut()
        .unwrap()
        .remove("completed_at");
    let ResponseEvent::Lifecycle { response, .. } = ResponseEvent::decode(wire).unwrap() else {
        panic!("lifecycle projection");
    };
    assert_eq!(response.completed_at, Field::Absent);
    let mut text = json!({
        "type":"response.output_text.delta","sequence_number":3,"item_id":"i",
        "output_index":0,"content_index":0,"delta":"secret",
        "logprobs":[{"token":"secret","logprob":-0.25,
            "top_logprobs":[{"token":"secret","logprob":-0.5,"future":true}],"future":true}]
    });
    let event = ResponseEvent::decode(text.clone()).unwrap();
    let ResponseEvent::OutputTextDelta { logprobs, .. } = event else {
        panic!("text delta");
    };
    assert_eq!(logprobs[0].logprob, -0.25);
    assert!(!format!("{:?}", logprobs[0]).contains("secret"));
    assert!(!format!("{:?}", logprobs[0].top_logprobs[0]).contains("secret"));
    text["logprobs"][0]["logprob"] = json!("-0.25");
    assert!(ResponseEvent::decode(text).is_err());
}
