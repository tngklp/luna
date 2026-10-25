//! End-to-end tests for the agent loop and the orchestrator.
//!
//! A scripted provider stands in for a real model, so these tests exercise the
//! real loop, the real tools and the real event stream without a network.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;

use luna_core::agent::{AgentDeps, AgentSession, UiAnswer, UiBridge, UiRequest};
use luna_core::config::{ApprovalAction, ApprovalConfig, Config, ProviderConfig, ProviderKind};
use luna_core::event::{AgentEvent, AgentRef, Emitted, EventSink};
use luna_core::orchestrator::Orchestrator;
use luna_core::provider::{ChatRequest, Provider, ProviderEvent, ProviderRegistry, ProviderStream};
use luna_core::tool::todo::{Task, TaskBoard, TaskStatus};
use luna_core::tool::{ToolClass, ToolRegistry};
use luna_core::types::{ContentBlock, Message, StopReason, ToolCall};

/// A provider that replays a fixed script of events, one entry per call.
struct ScriptedProvider {
    script: Mutex<VecDeque<Vec<ProviderEvent>>>,
    seen: Mutex<Vec<ChatRequest>>,
}

impl ScriptedProvider {
    fn new(script: Vec<Vec<ProviderEvent>>) -> Self {
        ScriptedProvider { script: Mutex::new(script.into()), seen: Mutex::new(Vec::new()) }
    }

    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn last_request(&self) -> Option<ChatRequest> {
        self.seen.lock().unwrap().last().cloned()
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn name(&self) -> &str {
        "scripted"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Openai
    }

    async fn stream(&self, request: ChatRequest) -> luna_core::Result<ProviderStream> {
        self.seen.lock().unwrap().push(request);
        let events = self.script.lock().unwrap().pop_front().unwrap_or_else(|| {
            vec![ProviderEvent::TextDelta("done".into()), ProviderEvent::Done(StopReason::EndTurn)]
        });
        Ok(Box::pin(futures_util::stream::iter(events.into_iter().map(Ok))))
    }
}

/// Records every event so tests can assert on the stream.
#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<serde_json::Value>>,
}

impl EventSink for Recorder {
    fn emit(&self, event: Emitted) {
        self.events.lock().unwrap().push(event.to_value());
    }
}

impl Recorder {
    fn types(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| event.get("type").and_then(|kind| kind.as_str()))
            .map(|kind| kind.to_string())
            .collect()
    }

    fn count(&self, kind: &str) -> usize {
        self.types().iter().filter(|name| name.as_str() == kind).count()
    }

    fn all(&self) -> Vec<serde_json::Value> {
        self.events.lock().unwrap().clone()
    }
}

fn config() -> Config {
    let mut config = Config::default();
    config.general.default_model = "scripted/test".into();
    config.general.max_steps = 8;
    config.providers.insert(
        "scripted".into(),
        ProviderConfig {
            preset: None,
            kind: ProviderKind::Openai,
            base_url: Some("http://localhost:1".into()),
            api_key: Some("test".into()),
            api_key_env: None,
            headers: Default::default(),
            models: vec!["test".into()],
            discover_models: false,
        },
    );
    config
}

fn deps(
    config: &Config,
    provider: Arc<ScriptedProvider>,
    sink: Arc<Recorder>,
    cwd: &std::path::Path,
) -> Arc<AgentDeps> {
    let registry = Arc::new(ProviderRegistry::new(config).with_provider("scripted", provider));
    Arc::new(AgentDeps::new(registry, sink, cwd))
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("luna-e2e-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn a_plain_turn_streams_text_and_settles() {
    let provider = Arc::new(ScriptedProvider::new(vec![vec![
        ProviderEvent::TextDelta("hello ".into()),
        ProviderEvent::TextDelta("world".into()),
        ProviderEvent::Done(StopReason::EndTurn),
    ]]));
    let sink = Arc::new(Recorder::default());
    let dir = temp_dir("plain");
    let mut session = AgentSession::new(
        deps(&config(), provider.clone(), sink.clone(), &dir),
        AgentRef::root("root"),
        "scripted/test",
        "system".into(),
    );

    session.run_turn("hi").await.unwrap();

    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.messages[1].text(), "hello world");
    assert_eq!(sink.count("message_start"), 1);
    assert_eq!(sink.count("message_end"), 1);
    assert_eq!(sink.count("agent_settled"), 1);
    assert_eq!(provider.calls(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn tool_calls_run_and_feed_results_back() {
    let dir = temp_dir("tools");
    std::fs::write(dir.join("note.txt"), "the answer is 42\n").unwrap();

    let provider = Arc::new(ScriptedProvider::new(vec![
        vec![
            ProviderEvent::ToolCallStart { index: 0, id: "call_1".into(), name: "read".into() },
            ProviderEvent::ToolCallArgsDelta { index: 0, delta: r#"{"path":"note.txt"}"#.into() },
            ProviderEvent::ToolCallEnd {
                index: 0,
                call: ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    arguments: json!({"path": "note.txt"}),
                },
            },
            ProviderEvent::Done(StopReason::ToolUse),
        ],
        vec![
            ProviderEvent::TextDelta("the answer is 42".into()),
            ProviderEvent::Done(StopReason::EndTurn),
        ],
    ]));

    let mut config = config();
    config.approvals = ApprovalConfig {
        read: ApprovalAction::Allow,
        write: ApprovalAction::Allow,
        bash: ApprovalAction::Allow,
        network: ApprovalAction::Allow,
    };

    let sink = Arc::new(Recorder::default());
    let mut session = AgentSession::new(
        deps(&config, provider.clone(), sink.clone(), &dir),
        AgentRef::root("root"),
        "scripted/test",
        "system".into(),
    );

    session.run_turn("read the note").await.unwrap();

    // user, assistant(tool call), tool result, assistant(answer)
    assert_eq!(session.messages.len(), 4);
    assert_eq!(session.messages[2].role, luna_core::types::Role::Tool);
    assert!(session.messages[2].text().contains("the answer is 42"));
    assert_eq!(session.messages[3].text(), "the answer is 42");

    assert_eq!(sink.count("tool_execution_start"), 1);
    assert_eq!(sink.count("tool_execution_end"), 1);
    assert_eq!(provider.calls(), 2);
    assert_eq!(session.stats.tool_calls, 1);

    // The second request must carry the tool result back to the model.
    let second = provider.last_request().unwrap();
    assert!(second.messages.iter().any(|message| message.role == luna_core::types::Role::Tool));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_denied_tool_reports_an_error_instead_of_running() {
    let dir = temp_dir("denied");
    std::fs::write(dir.join("secret.txt"), "do not touch\n").unwrap();

    let provider = Arc::new(ScriptedProvider::new(vec![
        vec![
            ProviderEvent::ToolCallEnd {
                index: 0,
                call: ToolCall {
                    id: "call_1".into(),
                    name: "write".into(),
                    arguments: json!({"path": "secret.txt", "content": "changed"}),
                },
            },
            ProviderEvent::Done(StopReason::ToolUse),
        ],
        vec![
            ProviderEvent::TextDelta("understood".into()),
            ProviderEvent::Done(StopReason::EndTurn),
        ],
    ]));

    let mut config = config();
    config.approvals.write = ApprovalAction::Deny;

    let sink = Arc::new(Recorder::default());
    let mut session = AgentSession::new(
        deps(&config, provider, sink, &dir),
        AgentRef::root("root"),
        "scripted/test",
        "system".into(),
    );

    session.run_turn("change the file").await.unwrap();

    // The file is untouched and the model was told why.
    assert_eq!(std::fs::read_to_string(dir.join("secret.txt")).unwrap(), "do not touch\n");
    let tool_result = session
        .messages
        .iter()
        .find(|message| message.role == luna_core::types::Role::Tool)
        .unwrap();
    assert!(tool_result.text().contains("denied"));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn steering_is_injected_into_the_running_turn() {
    let provider = Arc::new(ScriptedProvider::new(vec![
        vec![
            ProviderEvent::ToolCallEnd {
                index: 0,
                call: ToolCall { id: "c1".into(), name: "ls".into(), arguments: json!({}) },
            },
            ProviderEvent::Done(StopReason::ToolUse),
        ],
        vec![ProviderEvent::TextDelta("adjusted".into()), ProviderEvent::Done(StopReason::EndTurn)],
    ]));

    let mut config = config();
    config.approvals.read = ApprovalAction::Allow;

    let sink = Arc::new(Recorder::default());
    let dir = temp_dir("steer");
    let mut session = AgentSession::new(
        deps(&config, provider.clone(), sink.clone(), &dir),
        AgentRef::root("root"),
        "scripted/test",
        "system".into(),
    );

    session.steer("actually, look at src instead");
    session.run_turn("list the directory").await.unwrap();

    let second = provider.last_request().unwrap();
    assert!(
        second.messages.iter().any(|message| message.text().contains("look at src")),
        "the steering message should reach the model"
    );
    // One update when steering was queued, one when it was drained, and one
    // after the tool round.
    assert!(sink.count("queue_update") >= 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn aborting_a_turn_stops_it_cleanly() {
    let provider = Arc::new(ScriptedProvider::new(vec![vec![
        ProviderEvent::TextDelta("partial".into()),
        ProviderEvent::Done(StopReason::EndTurn),
    ]]));
    let sink = Arc::new(Recorder::default());
    let dir = temp_dir("abort");
    let mut session = AgentSession::new(
        deps(&config(), provider, sink.clone(), &dir),
        AgentRef::root("root"),
        "scripted/test",
        "system".into(),
    );

    session.abort();
    session.run_turn("hi").await.unwrap();
    assert_eq!(sink.count("agent_settled"), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn the_orchestrator_runs_a_subagent_and_reports_back() {
    let provider = Arc::new(ScriptedProvider::new(vec![vec![
        ProviderEvent::TextDelta("I changed two files.".into()),
        ProviderEvent::Done(StopReason::EndTurn),
    ]]));
    let sink = Arc::new(Recorder::default());
    let dir = temp_dir("orch");
    let config = config();
    let orchestrator = Orchestrator::new(
        deps(&config, provider, sink.clone(), &dir),
        config,
        AgentRef::root("root"),
    );

    let report = orchestrator
        .run_subagent(&AgentRef::root("root"), "worker", "Fix the parser", "Do the thing.", &dir)
        .await
        .unwrap();

    assert_eq!(report.status, luna_core::orchestrator::AgentStatus::Done);
    assert_eq!(report.summary, "I changed two files.");
    assert_eq!(report.role, "worker");

    // The tree and the event stream both know about the child.
    assert_eq!(orchestrator.tree().len(), 1);
    assert_eq!(sink.count("agent_spawned"), 1);
    assert_eq!(sink.count("agent_finished"), 1);
    let spawned =
        sink.all().into_iter().find(|event| event["type"] == json!("agent_spawned")).unwrap();
    assert_eq!(spawned["parentAgentId"], json!("root"));
    assert_eq!(spawned["depth"], json!(1));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn parallel_subagents_all_report() {
    let provider = Arc::new(ScriptedProvider::new(vec![
        vec![ProviderEvent::TextDelta("one".into()), ProviderEvent::Done(StopReason::EndTurn)],
        vec![ProviderEvent::TextDelta("two".into()), ProviderEvent::Done(StopReason::EndTurn)],
        vec![ProviderEvent::TextDelta("three".into()), ProviderEvent::Done(StopReason::EndTurn)],
    ]));
    let sink = Arc::new(Recorder::default());
    let dir = temp_dir("parallel");
    let mut config = config();
    config.general.max_concurrent_agents = 2;
    let orchestrator = Orchestrator::new(
        deps(&config, provider, sink.clone(), &dir),
        config,
        AgentRef::root("root"),
    );

    let jobs = vec![
        ("worker".to_string(), "A".to_string(), "do a".to_string()),
        ("worker".to_string(), "B".to_string(), "do b".to_string()),
        ("worker".to_string(), "C".to_string(), "do c".to_string()),
    ];
    let reports = orchestrator.run_parallel(&AgentRef::root("root"), jobs, &dir).await;

    assert_eq!(reports.len(), 3);
    assert!(reports
        .iter()
        .all(|report| report.status == luna_core::orchestrator::AgentStatus::Done));
    assert_eq!(sink.count("agent_spawned"), 3);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn the_board_drains_in_dependency_order() {
    let provider = Arc::new(ScriptedProvider::new(Vec::new()));
    let sink = Arc::new(Recorder::default());
    let dir = temp_dir("board");
    let config = config();
    let orchestrator = Orchestrator::new(
        deps(&config, provider, sink.clone(), &dir),
        config,
        AgentRef::root("root"),
    );

    let board = orchestrator.board();
    board.replace(vec![
        Task {
            id: "a".into(),
            title: "first".into(),
            description: String::new(),
            status: TaskStatus::Pending,
            depends_on: vec![],
            owner: None,
        },
        Task {
            id: "b".into(),
            title: "second".into(),
            description: String::new(),
            status: TaskStatus::Pending,
            depends_on: vec!["a".into()],
            owner: None,
        },
    ]);

    let reports = orchestrator.drain_board(&AgentRef::root("root"), &dir).await;
    assert_eq!(reports.len(), 2);
    assert!(board.all_done());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn approval_policy_is_honoured_by_class() {
    let config = ApprovalConfig {
        read: ApprovalAction::Allow,
        write: ApprovalAction::Ask,
        bash: ApprovalAction::Deny,
        network: ApprovalAction::Allow,
    };
    assert_eq!(ToolClass::Read.approval(&config), ApprovalAction::Allow);
    assert_eq!(ToolClass::Write.approval(&config), ApprovalAction::Ask);
    assert_eq!(ToolClass::Bash.approval(&config), ApprovalAction::Deny);
}

#[test]
fn the_standard_registry_exposes_every_tool() {
    let registry = ToolRegistry::standard();
    for name in [
        "read",
        "write",
        "edit",
        "multi_edit",
        "ls",
        "glob",
        "grep",
        "bash",
        "todo_write",
        "todo_read",
    ] {
        assert!(registry.get(name).is_some(), "missing tool {name}");
    }
}

#[test]
fn events_carry_the_owning_agent() {
    let emitted = Emitted::new(AgentRef::root("root"), AgentEvent::AgentSettled {});
    let value = emitted.to_value();
    assert_eq!(value["type"], json!("agent_settled"));
    assert_eq!(value["agentId"], json!("root"));
    assert_eq!(value["depth"], json!(0));
}

#[test]
fn child_agents_record_their_parent() {
    let parent = AgentRef::root("root");
    let child = AgentRef::child("root-worker-0", &parent, "worker");
    assert_eq!(child.parent_agent_id.as_deref(), Some("root"));
    assert_eq!(child.depth, 1);
    assert_eq!(child.role.as_deref(), Some("worker"));
}

#[test]
fn messages_expose_their_tool_calls() {
    let message = Message::assistant(vec![
        ContentBlock::text("thinking"),
        ContentBlock::ToolCall { id: "c1".into(), name: "read".into(), arguments: json!({}) },
    ]);
    assert_eq!(message.tool_calls().len(), 1);
    assert_eq!(message.tool_calls()[0].name, "read");
}

/// A UI bridge that answers every request the same way.
struct FixedUi(UiAnswer);

#[async_trait]
impl UiBridge for FixedUi {
    async fn ask(&self, _request: UiRequest) -> UiAnswer {
        self.0
    }
}

#[tokio::test]
async fn an_approving_ui_lets_a_write_through() {
    let dir = temp_dir("approve");
    let provider = Arc::new(ScriptedProvider::new(vec![
        vec![
            ProviderEvent::ToolCallEnd {
                index: 0,
                call: ToolCall {
                    id: "c1".into(),
                    name: "write".into(),
                    arguments: json!({"path": "new.txt", "content": "written"}),
                },
            },
            ProviderEvent::Done(StopReason::ToolUse),
        ],
        vec![ProviderEvent::TextDelta("done".into()), ProviderEvent::Done(StopReason::EndTurn)],
    ]));

    let mut config = config();
    config.approvals.write = ApprovalAction::Ask;

    let sink = Arc::new(Recorder::default());
    let registry = Arc::new(ProviderRegistry::new(&config).with_provider("scripted", provider));
    let deps =
        Arc::new(AgentDeps::new(registry, sink, &dir).with_ui(Arc::new(FixedUi(UiAnswer::Allow))));
    let mut session =
        AgentSession::new(deps, AgentRef::root("root"), "scripted/test", "system".into());

    session.run_turn("write the file").await.unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("new.txt")).unwrap(), "written");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_board_reports_deadlocks() {
    let board = TaskBoard::new();
    board.replace(vec![
        Task {
            id: "a".into(),
            title: "blocked".into(),
            description: String::new(),
            status: TaskStatus::Blocked,
            depends_on: vec![],
            owner: None,
        },
        Task {
            id: "b".into(),
            title: "waiting".into(),
            description: String::new(),
            status: TaskStatus::Pending,
            depends_on: vec!["a".into()],
            owner: None,
        },
    ]);
    assert_eq!(board.deadlocked().len(), 1);
    assert!(!board.all_done());
}
