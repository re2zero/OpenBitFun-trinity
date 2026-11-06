use super::*;
use crate::runtime::ToolStatus;
use crate::tabs;
use gpui_kit::Modifiers;
use openbitfun_core::service::i18n::LocaleId;
use std::cell::Cell;
use std::rc::Rc;

fn chat_with(entries: Vec<ChatEntry>, cx: &mut gpui_kit::TestAppContext) -> ChatView {
    let scroller = cx.new(|cx| MessageScrollerState::new(entries.len(), cx));
    ChatView {
        session_id: "session".into(),
        name: "name".into(),
        agent_type: "Standard".into(),
        model_id: None,
        entries,
        scroller,
        busy: false,
        submitting: false,
        child_context: None,
        expanded_user_texts: BTreeSet::new(),
        expanded_groups: BTreeMap::new(),
        turn_times: HashMap::new(),
        active_turn_id: None,
    }
}

#[gpui_kit::test]
fn append_chunk_reports_tail_growth_vs_new_entry(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| gpui_kit::init(cx));
    let mut chat = chat_with(Vec::new(), cx);
    assert!(chat.append_chunk("round-1", "hello "));
    assert!(!chat.append_chunk("round-1", "world"));
    assert!(chat.append_chunk("round-2", "next round"));
    assert_eq!(chat.entries.len(), 2);
}

fn turn_entries() -> Vec<ChatEntry> {
    vec![
        ChatEntry::User {
            text: "fix the bug".into(),
            turn_id: Some("turn-1".into()),
        },
        ChatEntry::Assistant {
            round_id: "round-1".into(),
            text: "first answer".into(),
        },
        ChatEntry::Tool(ToolCard {
            key: "tool-1".into(),
            call_id: "tool-1".into(),
            tool_name: "read_file".into(),
            status: ToolStatus::Done,
            facts: transcript::ToolFacts::empty(),
            result_success: None,
            subagent_session: None,
            background: false,
        }),
        ChatEntry::Assistant {
            round_id: "round-2".into(),
            text: "final answer".into(),
        },
    ]
}

#[test]
fn turn_footer_plan_hosts_the_finished_turn_tail() {
    let text = gpui_foundation::i18n::fallback_text(LocaleId::EnUS);
    let entries = turn_entries();
    // The tail assistant closes the finished turn; the copy scopes use
    // the web transcript shape (`User\n…` + rounds, `---` separated).
    let times = HashMap::new();
    let plan =
        turn_footer_plan(&entries, 3, false, false, &text, &times).expect("tail hosts footer");
    assert_eq!(plan.turn_id.as_deref(), Some("turn-1"));
    assert!(plan.meta.is_none(), "no timing row, no meta");
    assert_eq!(plan.copy_result.as_deref(), Some("final answer"));
    assert_eq!(
        plan.copy_full.as_deref(),
        Some(
            format!(
                "{}\nfix the bug\n\n---\n\nfirst answer\n\n---\n\nfinal answer",
                text.turn_copy_user_label
            )
            .as_str()
        )
    );
    // A turn closed by the next prompt is finished history.
    let mut history = turn_entries();
    history.push(ChatEntry::User {
        text: "next please".into(),
        turn_id: Some("turn-2".into()),
    });
    let times = HashMap::new();
    assert!(turn_footer_plan(&history, 3, true, false, &text, &times).is_some());
}

/// G-S1: the footer meta reads the turn's timing row — the runtime's own
/// duration wins over the wall-clock span, a stopped turn keeps its flag,
/// and a turn without a recorded end renders no meta.
#[test]
fn turn_footer_plan_reads_the_turn_meta() {
    let text = gpui_foundation::i18n::fallback_text(LocaleId::EnUS);
    let entries = turn_entries();
    let mut times = HashMap::new();
    times.insert(
        "turn-1".to_string(),
        runtime::TurnTimes {
            started_at_ms: Some(1_000),
            ended_at_ms: Some(13_000),
            duration_ms: None,
            cancelled: false,
        },
    );
    let plan =
        turn_footer_plan(&entries, 3, false, false, &text, &times).expect("tail hosts footer");
    let meta = plan.meta.expect("a timed turn yields meta");
    assert_eq!(meta.completed_at_ms, 13_000);
    assert_eq!(meta.duration_ms, Some(12_000));
    assert!(!meta.cancelled);

    times.insert(
        "turn-1".to_string(),
        runtime::TurnTimes {
            started_at_ms: Some(1_000),
            ended_at_ms: Some(13_000),
            duration_ms: Some(9_500),
            cancelled: true,
        },
    );
    let plan =
        turn_footer_plan(&entries, 3, false, false, &text, &times).expect("tail hosts footer");
    let meta = plan.meta.expect("a timed turn yields meta");
    assert_eq!(meta.duration_ms, Some(9_500), "runtime duration wins");
    assert!(meta.cancelled);

    times.insert("turn-1".to_string(), runtime::TurnTimes::default());
    let plan =
        turn_footer_plan(&entries, 3, false, false, &text, &times).expect("tail hosts footer");
    assert!(plan.meta.is_none(), "no end recorded, no meta");
}

#[test]
fn turn_footer_plan_rejects_streaming_and_mid_turn_rows() {
    let text = gpui_foundation::i18n::fallback_text(LocaleId::EnUS);
    let entries = turn_entries();
    // The executing turn's tail stays footer-less until it finishes.
    let times = HashMap::new();
    assert!(turn_footer_plan(&entries, 3, true, false, &text, &times).is_none());
    assert!(turn_footer_plan(&entries, 3, false, true, &text, &times).is_none());
    // Tool cards inside the turn are not the tail; prompts and
    // system rows never host the footer.
    assert!(turn_footer_plan(&entries, 2, false, false, &text, &times).is_none());
    assert!(turn_footer_plan(&entries, 0, false, false, &text, &times).is_none());
    // A tool-only turn has no assistant text: the tail row still hosts
    // fork/snapshots while both copy scopes hide.
    let tool_only = vec![
        ChatEntry::User {
            text: "run it".into(),
            turn_id: Some("turn-1".into()),
        },
        ChatEntry::Tool(ToolCard {
            key: "tool-1".into(),
            call_id: "tool-1".into(),
            tool_name: "run_command".into(),
            status: ToolStatus::Done,
            facts: transcript::ToolFacts::empty(),
            result_success: None,
            subagent_session: None,
            background: false,
        }),
    ];
    let plan =
        turn_footer_plan(&tool_only, 1, false, false, &text, &times).expect("tool tail hosts");
    assert_eq!(plan.turn_id.as_deref(), Some("turn-1"));
    assert!(plan.copy_full.is_none() && plan.copy_result.is_none());
}

#[gpui_kit::test]
fn upsert_tool_reports_changed_index_or_append(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| gpui_kit::init(cx));
    let card = |key: &'static str| ToolCard {
        key: SharedString::from(key),
        call_id: key.to_string(),
        tool_name: "read_file".into(),
        status: ToolStatus::Running,
        facts: transcript::ToolFacts::empty(),
        result_success: None,
        subagent_session: None,
        background: false,
    };
    let mut chat = chat_with(Vec::new(), cx);
    assert!(chat.upsert_tool(card("tool-1")).is_none());
    assert!(chat.upsert_tool(card("tool-2")).is_none());
    assert_eq!(chat.upsert_tool(card("tool-1")), Some(0));
    assert!(chat.upsert_tool(card("tool-3")).is_none());
    assert_eq!(chat.entries.len(), 3);
}

#[test]
fn btw_question_parses_the_command_prefix() {
    assert_eq!(btw_question("/btw what is this?"), Some("what is this?"));
    assert_eq!(btw_question("/btw"), Some(""));
    assert_eq!(btw_question("  /btw   spaced  "), Some("spaced"));
    // "/btwfoo" is a plain message, not the command.
    assert_eq!(btw_question("/btwfoo"), None);
    assert_eq!(btw_question("plain question"), None);
}

#[test]
fn quick_action_text_resolves_builtins_and_keeps_customizations() {
    let text = gpui_foundation::i18n::fallback_text(LocaleId::EnUS);
    let action = |id: &str, label: &str, prompt: &str| {
        openbitfun_core::service::config::types::AiExperienceQuickAction {
            id: id.to_string(),
            label: label.to_string(),
            prompt: prompt.to_string(),
            enabled: true,
        }
    };

    // Built-in text still equal to the shared English defaults resolves
    // to the localized copy (with the English fallback both are equal).
    let (label, prompt) = resolve_quick_action_text(
        &action("commit", "Commit", "Commit all current code changes"),
        &text,
    );
    assert_eq!(label, text.quick_action_commit_label);
    assert_eq!(prompt, text.quick_action_commit_prompt);
    let (label, prompt) = resolve_quick_action_text(
        &action(
            "create_pr",
            "Create PR",
            "Create a Pull Request for the current branch",
        ),
        &text,
    );
    assert_eq!(label, text.quick_action_create_pr_label);
    assert_eq!(prompt, text.quick_action_create_pr_prompt);

    // Customized stored text stays verbatim (label and prompt resolve
    // independently, like the web helper).
    let (label, prompt) = resolve_quick_action_text(
        &action("commit", "Ship it", "Commit all current code changes"),
        &text,
    );
    assert_eq!(label, "Ship it");
    assert_eq!(prompt, text.quick_action_commit_prompt);

    // Unknown ids are custom actions: always verbatim.
    let (label, prompt) =
        resolve_quick_action_text(&action("deploy", "Deploy", "Ship to staging"), &text);
    assert_eq!(label, "Deploy");
    assert_eq!(prompt, "Ship to staging");
}

/// The M20 child-view loop projects onto the shell: a linked subagent
/// annotates the parent tool card, the card entry point opens the child
/// with a settled child context, and returning to the parent re-annotates
/// the reloaded transcript from the link map. `/btw` rides the same path.
#[gpui_kit::test]
fn subagent_and_btw_child_views_project_onto_the_shell(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    let opened = |session_id: &str, name: &str, entries: Vec<ChatEntry>| {
        RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
            reload: false,
            session_id: session_id.into(),
            name: name.into(),
            agent_type: "Standard".into(),
            model_id: None,
            entries,
            busy: false,
            turn_times: Default::default(),
        })
    };
    let parent_entries = || {
        vec![ChatEntry::Tool(crate::runtime::ToolCard {
            key: "tool-call-1".into(),
            call_id: "call-1".into(),
            tool_name: "task".into(),
            status: ToolStatus::Running,
            facts: transcript::ToolFacts::empty(),
            result_success: None,
            subagent_session: None,
            background: false,
        })]
    };

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(opened("parent", "Parent", parent_entries()), window, cx);
            // The runtime links the hidden child to that tool call.
            shell.apply_runtime_event(
                RuntimeEvent::SubagentLinked {
                    parent_session_id: "parent".into(),
                    parent_tool_call_id: "call-1".into(),
                    subagent_session_id: "child-1".into(),
                    agent_type: Some("Explore".into()),
                },
                window,
                cx,
            );
        });
    });
    window.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.subagent_links.len(), 1);
        let chat = shell.active_chat().unwrap();
        match &chat.entries[0] {
            ChatEntry::Tool(card) => {
                assert_eq!(card.subagent_session.as_deref(), Some("child-1"))
            }
            _ => panic!("expected a tool card"),
        }
    });

    // The card entry point opens the child; the opened echo settles the
    // subagent child context onto the pane.
    window.update(|_window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_subagent_run("child-1".into(), "parent".into(), cx);
        });
    });
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(opened("child-1", "child", Vec::new()), window, cx);
        });
    });
    window.update(|_, cx| {
        let chat = shell.read(cx).active_chat().unwrap();
        assert_eq!(chat.session_id, "child-1");
        let child = chat.child_context.as_ref().expect("child context set");
        assert_eq!(child.kind, ChildKind::Subagent);
        assert_eq!(child.agent_type.as_deref(), Some("Explore"));
    });

    // Returning to the parent re-annotates the reloaded card.
    window.update(|_window, cx| {
        shell.update(cx, |shell, cx| shell.return_from_child(cx));
    });
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(opened("parent", "Parent", parent_entries()), window, cx);
        });
    });
    window.update(|_, cx| {
        let chat = shell.read(cx).active_chat().unwrap();
        assert_eq!(chat.session_id, "parent");
        assert!(chat.child_context.is_none());
        match &chat.entries[0] {
            ChatEntry::Tool(card) => {
                assert_eq!(card.subagent_session.as_deref(), Some("child-1"))
            }
            _ => panic!("expected a tool card"),
        }
    });

    // `/btw` rides the same child machinery with its own badge kind; the
    // dialog turn id rides along so the stop button can target it.
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::BtwStarted {
                    parent_session_id: "parent".into(),
                    child_session_id: "btw-1".into(),
                    turn_id: "turn-1".into(),
                },
                window,
                cx,
            );
            shell.apply_runtime_event(opened("btw-1", "BTW", Vec::new()), window, cx);
        });
    });
    window.update(|_, cx| {
        let chat = shell.read(cx).active_chat().unwrap();
        assert_eq!(chat.session_id, "btw-1");
        let child = chat.child_context.as_ref().expect("btw child context");
        assert_eq!(child.kind, ChildKind::Btw);
        assert_eq!(child.parent_session_id, "parent");
        assert_eq!(child.btw_turn_id.as_deref(), Some("turn-1"));
    });
}

#[test]
fn list_sync_ranges_match_entry_counts() {
    assert_eq!(tail_changed_range(1), 0..1);
    assert_eq!(tail_changed_range(40), 39..40);
}

fn sample_usage_stats() -> openbitfun_services_core::token_usage::UsageStatistics {
    openbitfun_services_core::token_usage::UsageStatistics {
        total_requests: 3,
        unknown_usage_requests: 0,
        failed_requests: 0,
        by_account: Vec::new(),
        by_channel: Vec::new(),
        recent_attempts: Vec::new(),
        attempts_truncated: false,
        total_tokens: 300,
        total_input_tokens: 200,
        total_output_tokens: 100,
        total_cached_tokens: 50,
        total_cache_write_tokens: 0,
        total_cache_reported_input_tokens: 200,
        by_model: Vec::new(),
        by_group: Vec::new(),
        by_endpoint: Vec::new(),
        trend: Vec::new(),
        granularity: openbitfun_services_core::token_usage::UsageGranularity::Day,
    }
}

/// M22 pump projections: the update state machine, the launch-at-login
/// global, and the settings-page data events land on the window in the
/// shape the general/usage/web-search pages render.
#[gpui_kit::test]
fn m22_settings_events_project_onto_the_window(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            // An offered release becomes Available; the install finishing
            // moves to Installed; a failed check surfaces its message.
            shell.apply_runtime_event(
                RuntimeEvent::UpdateCheckFinished {
                    result: Ok(gpui_host::update::UpdateCheckReport {
                        offered_version: Some("9.9.9".into()),
                        unsupported_reason: None,
                    }),
                },
                window,
                cx,
            );
            assert_eq!(
                shell.update_state,
                gpui_host::update::UpdateUiState::Available {
                    version: "9.9.9".into()
                }
            );
            shell.apply_runtime_event(
                RuntimeEvent::UpdateInstallFinished {
                    result: Ok("9.9.9".into()),
                },
                window,
                cx,
            );
            assert_eq!(
                shell.update_state,
                gpui_host::update::UpdateUiState::Installed {
                    version: "9.9.9".into()
                }
            );
            shell.apply_runtime_event(
                RuntimeEvent::UpdateCheckFinished {
                    result: Err("offline".into()),
                },
                window,
                cx,
            );
            assert_eq!(
                shell.update_state,
                gpui_host::update::UpdateUiState::Failed {
                    message: "offline".into()
                }
            );

            // Launch-at-login state lands in its global for the toggle.
            shell.apply_runtime_event(RuntimeEvent::AutostartState(true), window, cx);
            assert!(
                cx.try_global::<settings::AutostartState>()
                    .expect("autostart global")
                    .0
            );

            // The usage snapshot replaces the loading flag.
            shell.usage_loading = true;
            shell.apply_runtime_event(
                RuntimeEvent::UsageReady(Ok(Box::new(sample_usage_stats()))),
                window,
                cx,
            );
            assert!(!shell.usage_loading);
            assert!(shell.usage_stats.is_some());

            // The credential status arrives with its provider attached.
            shell.apply_runtime_event(
                RuntimeEvent::WebSearchStatus(Ok(("tavily".into(), true))),
                window,
                cx,
            );
            assert_eq!(shell.websearch_status, Some(("tavily".to_string(), true)));
        });
    });
}

/// A harness view mirroring `ShellWindow::render_chat_area` so the test
/// exercises the same scroller construction and item renderer.
struct TranscriptListHarness {
    scroller: Entity<MessageScrollerState>,
    entries: Rc<Vec<ChatEntry>>,
    render_count: Rc<Cell<usize>>,
    text: ShellText,
}

impl Render for TranscriptListHarness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let render_count = self.render_count.clone();
        let text = self.text.clone();
        MessageScroller::new(
            "transcript",
            self.scroller.clone(),
            move |index, _window, cx| {
                render_count.set(render_count.get() + 1);
                match this.read(cx).entries.get(index).cloned() {
                    Some(entry) => {
                        render_chat_item(&entry, &text, None, None, &[], index, false, cx)
                    }
                    None => div().into_any_element(),
                }
            },
        )
        .w(px(800.))
        .h(px(600.))
    }
}

#[gpui_kit::test]
fn management_catalogs_project_onto_the_shell(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SkillCatalog(crate::manage::SkillCatalog {
                    loaded: true,
                    skills: vec![crate::manage::SkillRow {
                        key: "user::demo".into(),
                        name: "Demo".into(),
                        description: "demo skill".into(),
                        project_level: false,
                        builtin: false,
                        shadowed: false,
                        disabled: true,
                        group_key: None,
                        source_id: String::new(),
                        source_label: String::new(),
                        path: String::new(),
                        deletable: false,
                    }],
                }),
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::McpCatalog(crate::manage::McpCatalog {
                    loaded: true,
                    servers: vec![crate::manage::McpServerRow {
                        id: "demo".into(),
                        name: "Demo".into(),
                        remote: false,
                        target: "demo-server --stdio".into(),
                        state: crate::manage::McpServerState::Failed,
                        state_message: Some("spawn failed".into()),
                        enabled: true,
                        auto_start: true,
                        builtin: false,
                        auth_configured: false,
                        auth_source: None,
                        oauth_enabled: false,
                    }],
                }),
                window,
                cx,
            );
            // A failure outside a dialog lands in the banner state.
            shell.apply_runtime_event(RuntimeEvent::ManageFailed("boom".into()), window, cx);
        });
    });
    window.update(|_, cx| {
        let skills = cx
            .try_global::<crate::manage::SkillCatalog>()
            .expect("skill catalog global");
        assert_eq!(skills.skills.len(), 1);
        assert!(skills.skills[0].disabled);

        let servers = cx
            .try_global::<crate::manage::McpCatalog>()
            .expect("mcp catalog global");
        assert_eq!(servers.servers.len(), 1);
        assert_eq!(
            servers.servers[0].state,
            crate::manage::McpServerState::Failed
        );

        assert!(crate::manage::ManageNotice::current(cx).is_some());
    });
}

/// G-A6/G-A3: the agents page renders the harness zone from the catalog
/// (four profile cards) and keeps the four builtin harness profiles out of
/// the modes list, while a subagent row carries its model menu.
#[gpui_kit::test]
fn harness_zone_and_subagent_model_menu_lay_out(cx: &mut gpui_kit::TestAppContext) {
    use openbitfun_core::service::config::types::SubagentModelSelection;
    install_shell_globals(cx);
    let text = sample_shell_text();
    let card = |id: &str, source: crate::manage::AgentSourceKind, subagent: bool| {
        crate::manage::AgentCardRow {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            tool_count: 1,
            readonly: false,
            review: false,
            source,
            enabled: subagent.then_some(true),
            skills_manageable: false,
            skills_mode_id: id.to_string(),
            model_override: subagent.then_some(SubagentModelSelection::Inherit),
            supports_follow_up: true,
            external_provider_label: None,
            default_tools: Vec::new(),
            profile_id: id.to_string(),
            profile_label: None,
            profile_member_ids: vec![id.to_string()],
        }
    };
    let catalog = crate::manage::AgentCatalog {
        loaded: true,
        modes: vec![
            card("Minimal", crate::manage::AgentSourceKind::Builtin, false),
            card("Standard", crate::manage::AgentSourceKind::Builtin, false),
            card("Ultimate", crate::manage::AgentSourceKind::Builtin, false),
            card("Creative", crate::manage::AgentSourceKind::Builtin, false),
            card("Claw", crate::manage::AgentSourceKind::Builtin, false),
        ],
        subagents: vec![card("Explorer", crate::manage::AgentSourceKind::User, true)],
    };
    cx.update(|cx| {
        cx.set_global(catalog);
        cx.set_global(crate::runtime::ModelCatalog {
            loaded: true,
            models: vec![crate::runtime::ModelChoice {
                id: "claude-sonnet".to_string(),
                name: "Claude Sonnet".to_string(),
                provider: "anthropic".to_string(),
                enabled: true,
            }],
        });
    });
    let (ttx, trx) = mpsc::unbounded_channel();
    let shell_view = cx.add_empty_window().update(|window, cx| {
        cx.new(|cx| ShellWindow::new(ttx, trx, window, cx))
            .downgrade()
    });
    let view = shell_view.clone();
    {
        let window = cx.add_empty_window();
        window.draw(point(px(0.), px(0.)), size(px(1100.), px(900.)), {
            let text = text.clone();
            let view = view.clone();
            move |_, cx| {
                cx.new(|_| SettingsHarness {
                    text: text.clone(),
                    view: view.clone(),
                    page: settings::SettingsEntry::Agents,
                })
                .into_any_element()
            }
        });
        window.run_until_parked();
        for selector in [
            "manage-harness-zone",
            "manage-harness-Minimal",
            "manage-harness-Standard",
            "manage-harness-Ultimate",
            "manage-harness-Creative",
        ] {
            assert!(
                window.debug_bounds(selector).is_some(),
                "the harness zone renders {selector}"
            );
        }
        let zone = window
            .debug_bounds("manage-harness-zone")
            .expect("harness zone bounds");
        // Four cards share the grid row: their left edges are distinct and
        // every card sits inside the zone.
        let mut lefts: Vec<f32> = Vec::new();
        for (profile, selector) in [
            ("Minimal", "manage-harness-Minimal"),
            ("Standard", "manage-harness-Standard"),
            ("Ultimate", "manage-harness-Ultimate"),
            ("Creative", "manage-harness-Creative"),
        ] {
            let bounds = window.debug_bounds(selector).expect("card bounds");
            assert!(bounds.left() >= zone.left(), "{profile} inside the zone");
            assert!(
                bounds.bottom() <= zone.bottom(),
                "{profile} inside the zone height"
            );
            lefts.push(bounds.left().into());
        }
        lefts.sort_by(f32::total_cmp);
        lefts.dedup();
        assert_eq!(lefts.len(), 4, "the four profiles lay out side by side");
    }
    // The subagent list only renders when the type filter keeps it (the
    // settings pane virtualizes below the fold), so the menu is asserted
    // on that view.
    cx.update(|cx| {
        cx.set_global(crate::manage::AgentsFilterState {
            kind: crate::manage::AgentKindFilter::SubAgent,
            ..Default::default()
        });
    });
    let window = cx.add_empty_window();
    window.draw(point(px(0.), px(0.)), size(px(1100.), px(900.)), {
        let text = text.clone();
        let view = view.clone();
        move |_, cx| {
            cx.new(|_| SettingsHarness {
                text: text.clone(),
                view: view.clone(),
                page: settings::SettingsEntry::Agents,
            })
            .into_any_element()
        }
    });
    window.run_until_parked();
    assert!(
        window.debug_bounds("agent-model-Explorer").is_some(),
        "the subagent row carries its model menu"
    );
    assert!(
        window.debug_bounds("manage-harness-zone").is_some(),
        "the harness zone stays above the filtered catalog"
    );
}

#[gpui_kit::test]
fn mcp_json_and_resource_events_land_in_window_state(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            // A failed save keeps the editor open with the inline error.
            shell.apply_runtime_event(
                RuntimeEvent::McpJsonSaved(Err("JSON parsing failed".into())),
                window,
                cx,
            );
            // A resource reply lands on the matching pending read.
            shell.mcp_resource_view = Some(crate::manage::McpResourceView {
                uri: "file:///a.txt".into(),
                loading: true,
                result: None,
            });
            shell.apply_runtime_event(
                RuntimeEvent::McpResourceRead(Box::new(crate::manage::McpResourceReadOutcome {
                    uri: "file:///a.txt".into(),
                    result: Ok(
                        openbitfun_core::service::mcp::protocol::ResourcesReadResult {
                            contents: vec![
                                openbitfun_core::service::mcp::protocol::MCPResourceContent {
                                    uri: "file:///a.txt".into(),
                                    content: Some("hello".into()),
                                    blob: None,
                                    mime_type: Some("text/plain".into()),
                                    annotations: None,
                                    meta: None,
                                },
                            ],
                        },
                    ),
                })),
                window,
                cx,
            );
        });
    });
    window.update(|_, cx| {
        shell.update(cx, |shell, _cx| {
            assert!(shell.mcp_json_error.is_some());
            let view = shell.mcp_resource_view.as_ref().expect("resource view");
            assert!(!view.loading);
            let result = view.result.as_ref().expect("resource result");
            assert_eq!(
                crate::manage::render_resource_content(
                    result.as_ref().expect("read ok"),
                    &sample_shell_text()
                ),
                "[text/plain]\n\nhello"
            );
        });
    });
}

fn sample_shell_text() -> ShellText {
    gpui_foundation::i18n::fallback_text(LocaleId::EnUS)
}

/// A configured ACP client for the settings page tests (E3-B5).
fn acp_test_client(id: &str, name: &str) -> openbitfun_acp::client::AcpClientInfo {
    openbitfun_acp::client::AcpClientInfo {
        id: id.to_string(),
        name: name.to_string(),
        command: id.to_string(),
        args: Vec::new(),
        enabled: true,
        readonly: false,
        subagent: openbitfun_acp::client::AcpClientSubagentConfig::default(),
        permission_mode: openbitfun_acp::client::AcpClientPermissionMode::Ask,
        status: openbitfun_acp::client::AcpClientStatus::Configured,
        tool_name: id.to_string(),
        session_count: 0,
    }
}

/// A requirement probe for one ACP agent (E3-B5).
fn acp_test_probe(id: &str, installed: bool) -> openbitfun_acp::client::AcpClientRequirementProbe {
    openbitfun_acp::client::AcpClientRequirementProbe {
        id: id.to_string(),
        tool: openbitfun_acp::client::AcpRequirementProbeItem {
            name: id.to_string(),
            installed,
            version: None,
            path: None,
            error: None,
        },
        adapter: None,
        runnable: installed,
        notes: Vec::new(),
    }
}

fn permission_item(request_id: &str) -> PermissionPromptItem {
    PermissionPromptItem {
        request: crate::runtime::test_permission_request(request_id),
        resources_line: Some("Resources: resource-a".into()),
        project_line: "Project: project".into(),
        delegation_line: None,
        risk_line: None,
    }
}

#[test]
fn permission_queue_resolution_drains_by_request_id() {
    let mut queue = vec![permission_item("req-a"), permission_item("req-b")];
    queue.retain(|item| item.request_id() != "req-a");
    assert_eq!(queue.len(), 1);
    queue.retain(|item| item.request_id() != "req-b");
    assert!(queue.is_empty());
}

/// Composition happens at arrival; without the i18n service initialized
/// the interpolating subagent label falls back to the raw key, which keeps
/// the assertion deterministic.
#[gpui_kit::test]
fn permission_item_composes_context_lines(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        let text = sample_shell_text();
        let mut request = crate::runtime::test_permission_request("req-1");
        request.project_path = Some("/tmp/ws".into());
        let item = compose_permission_item(request, &text, cx);
        assert_eq!(item.project_line, "Project: /tmp/ws");
        assert_eq!(
            item.resources_line.as_deref(),
            Some("Resources: resource-a")
        );
        assert!(item.delegation_line.is_none());
        assert!(item.risk_line.is_none());

        let mut delegated = crate::runtime::test_permission_request("req-2");
        delegated.display_metadata = Default::default();
        let delegated_item = compose_permission_item(delegated, &text, cx);
        assert_eq!(delegated_item.project_line, "Project: project");
    });
}

/// Alternating user/assistant pairs with markdown of varying length, plus
/// a fenced code block every tenth round.
fn stress_entries(rounds: usize) -> Vec<ChatEntry> {
    let mut entries = Vec::with_capacity(rounds * 2);
    for index in 0..rounds {
        entries.push(ChatEntry::User {
            text: format!("Stress round {index}: please explain item {index}."),
            turn_id: None,
        });
        let paragraph = format!(
            "Answer **{index}** with a [link](https://example.com/{index}) and `inline code`.\n\n\
             - first point for round {index}\n- second point with a longer line of text that wraps\n\n\
             | column a | column b |\n|---|---|\n| {index} | value |"
        );
        let body = if index % 10 == 0 {
            format!(
                "{paragraph}\n\n```rust\nfn round_{index}() -> usize {{\n    {index} * 2\n}}\n```"
            )
        } else {
            paragraph
        };
        entries.push(ChatEntry::Assistant {
            round_id: format!("round-{index}"),
            text: body,
        });
    }
    entries
}

#[gpui_kit::test]
fn transcript_list_renders_visible_range_only(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| gpui_kit::init(cx));

    const ROUNDS: usize = 500;
    let entries = Rc::new(stress_entries(ROUNDS));
    let render_count: Rc<Cell<usize>> = Default::default();
    let scroller = cx.new(|cx| MessageScrollerState::new(entries.len(), cx));

    let window = cx.add_empty_window();
    let draw_frame = |window: &mut gpui_kit::VisualTestContext,
                      scroller: &Entity<MessageScrollerState>,
                      entries: &Rc<Vec<ChatEntry>>,
                      render_count: &Rc<Cell<usize>>|
     -> std::time::Duration {
        render_count.set(0);
        let scroller = scroller.clone();
        let entries = entries.clone();
        let render_count = render_count.clone();
        let text = sample_shell_text();
        let started = std::time::Instant::now();
        window.draw(
            point(px(0.), px(0.)),
            size(px(800.), px(600.)),
            move |_, cx| {
                cx.new(|_| TranscriptListHarness {
                    scroller,
                    entries,
                    render_count,
                    text,
                })
                .into_any_element()
            },
        );
        started.elapsed()
    };

    // Transcript restore: the first frame must neither render nor measure
    // all 1k entries (markdown parse only happens for rendered items).
    draw_frame(window, &scroller, &entries, &render_count);
    let restored = render_count.get();
    assert!(
        restored < 100,
        "restore rendered {restored} of {} entries in one frame",
        entries.len()
    );

    // Sweep the whole list top to bottom by jumping row anchors; every
    // frame stays bounded while the scroller re-measures the landed range.
    let mut max_per_frame = 0usize;
    let mut total = std::time::Duration::ZERO;
    let steps = 60;
    let total_entries = entries.len();
    for step in 0..steps {
        let anchor = step * (total_entries.saturating_sub(1)) / steps;
        window.update(|_, cx| {
            scroller.update(cx, |scroller, cx| {
                scroller.scroll_to_item(anchor, cx);
            });
        });
        window.run_until_parked();
        let elapsed = draw_frame(window, &scroller, &entries, &render_count);
        max_per_frame = max_per_frame.max(render_count.get());
        total += elapsed;
    }
    assert!(
        max_per_frame < 100,
        "a scroll frame rendered {max_per_frame} entries"
    );
    let average_us = total.as_micros() / steps as u128;
    println!(
        "transcript sweep: {} entries, {steps} frames, max {max_per_frame} rendered/frame, \
         {average_us} us average frame layout",
        entries.len()
    );

    // Streaming growth on top of the restored transcript keeps item
    // accounting exact.
    let mut grown = entries.as_ref().clone();
    for round in 0..5 {
        grown.push(ChatEntry::User {
            text: format!("live round {round}"),
            turn_id: None,
        });
        window.update(|_, cx| {
            scroller.update(cx, |scroller, cx| {
                scroller.append(1, cx);
            });
        });
        grown.push(ChatEntry::Assistant {
            round_id: format!("live-{round}"),
            text: "streamed **markdown** body.\n\n- item\n- item".to_string(),
        });
        let count = grown.len();
        window.update(|_, cx| {
            scroller.update(cx, |scroller, cx| {
                scroller.append(1, cx);
                scroller.remeasure_items(tail_changed_range(count), cx);
            });
        });
    }
    window.update(|_, cx| {
        scroller.update(cx, |scroller, _| {
            assert_eq!(scroller.item_count(), grown.len());
        });
    });
    let entries = Rc::new(grown);
    draw_frame(window, &scroller, &entries, &render_count);
    assert!(
        render_count.get() < 100,
        "post-growth frame rendered {} entries",
        render_count.get()
    );
}

/// The settings page must build against the shell globals (theme, shell
/// config mirror, locale copy) without a live runtime.
#[gpui_kit::test]
fn settings_page_renders_headlessly(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let presets = theme::load_presets();
        let active = presets
            .iter()
            .position(|preset| preset.id == DEFAULT_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active) {
            preset.apply(cx);
        }
        cx.set_global(ShellState {
            presets,
            active_preset: active,
        });
        cx.set_global(settings::ShellConfig::startup_default(
            openbitfun_core::service::i18n::LocaleId::ZhCN,
        ));
        cx.set_global(sample_shell_text());
    });
    let window = cx.add_empty_window();
    let text = sample_shell_text();
    let (ttx, trx) = mpsc::unbounded_channel();
    let shell_view = window.update(|window, cx| {
        cx.new(|cx| ShellWindow::new(ttx, trx, window, cx))
            .downgrade()
    });
    window.draw(
        point(px(0.), px(0.)),
        size(px(900.), px(600.)),
        move |_, cx| {
            // The page must render inside a view: the component library
            // asks the window for the current view id during layout.
            cx.new(|_| SettingsHarness {
                text: text.clone(),
                view: shell_view.clone(),
                page: settings::SettingsEntry::General,
            })
            .into_any_element()
        },
    );
    window.run_until_parked();
}

/// A2: the pages that gained fields (appearance font size, model
/// timeouts/proxy, execution timeouts/subagents/tool switches/review
/// capacity) build and lay out against the shell globals. Rendering the
/// page runs every field's getter, so a broken read shows up here rather
/// than in the Xvfb smoke; the section headings carry the selectors.
#[gpui_kit::test]
fn settings_new_field_pages_lay_out(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let presets = theme::load_presets();
        let active = presets
            .iter()
            .position(|preset| preset.id == DEFAULT_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active) {
            preset.apply(cx);
        }
        cx.set_global(ShellState {
            presets,
            active_preset: active,
        });
        cx.set_global(settings::ShellConfig::startup_default(LocaleId::ZhCN));
        cx.set_global(sample_shell_text());
    });
    let text = sample_shell_text();
    let (ttx, trx) = mpsc::unbounded_channel();
    let shell_view = cx.add_empty_window().update(|window, cx| {
        cx.new(|cx| ShellWindow::new(ttx, trx, window, cx))
            .downgrade()
    });

    // The manage pages read their catalogs as globals; seed the minimum
    // the skills rail and the agents cards render from (the four builtin
    // harness profiles plus one regular mode and one subagent).
    cx.update(|cx| {
        cx.set_global(crate::manage::SkillCatalog {
            loaded: true,
            skills: vec![crate::manage::SkillRow {
                key: "user::demo".into(),
                name: "Demo".into(),
                description: "demo skill".into(),
                project_level: false,
                builtin: false,
                shadowed: false,
                disabled: false,
                group_key: None,
                source_id: String::new(),
                source_label: String::new(),
                path: String::new(),
                deletable: false,
            }],
        });
        let card = |id: &str, source: crate::manage::AgentSourceKind, subagent: bool| {
            crate::manage::AgentCardRow {
                id: id.to_string(),
                name: id.to_string(),
                description: String::new(),
                tool_count: 1,
                readonly: false,
                review: false,
                source,
                enabled: subagent.then_some(true),
                skills_manageable: false,
                skills_mode_id: id.to_string(),
                model_override: None,
                supports_follow_up: true,
                external_provider_label: None,
                default_tools: Vec::new(),
                profile_id: id.to_string(),
                profile_label: None,
                profile_member_ids: vec![id.to_string()],
            }
        };
        cx.set_global(crate::manage::AgentCatalog {
            loaded: true,
            modes: vec![
                card("Minimal", crate::manage::AgentSourceKind::Builtin, false),
                card("Standard", crate::manage::AgentSourceKind::Builtin, false),
                card("Ultimate", crate::manage::AgentSourceKind::Builtin, false),
                card("Creative", crate::manage::AgentSourceKind::Builtin, false),
                card("Coder", crate::manage::AgentSourceKind::Builtin, false),
            ],
            subagents: vec![card("Explorer", crate::manage::AgentSourceKind::User, true)],
        });
        cx.set_global(crate::runtime::ModelCatalog {
            loaded: true,
            models: Vec::new(),
        });
        // E3-B5: the ACP page reads its catalog and page state as globals;
        // seed one configured agent (probed) so the registry row renders.
        cx.set_global(crate::acp::AcpCatalog {
            loaded: true,
            clients: vec![acp_test_client("codex", "Codex")],
            probes: vec![acp_test_probe("codex", true)],
            probing: false,
        });
        cx.set_global(crate::acp::AcpPageState::default());
    });

    let cases: &[(settings::SettingsEntry, &[&str])] = &[
        // The appearance page renders through the card path: its two
        // cards carry the anchors now (the interface-language card moved
        // back to 通用, where the web page keeps that section).
        (
            settings::SettingsEntry::Appearance,
            &[
                "settings-card-appearance-theme",
                "settings-card-appearance-font",
            ],
        ),
        // G-SE7: the shortcuts table renders its sections and rows; the
        // binding cross-check has its own test. The page is on the card
        // path, so the cards carry the anchors.
        (
            settings::SettingsEntry::Shortcuts,
            &[
                "settings-card-shortcuts-global",
                "settings-card-shortcuts-terminal",
            ],
        ),
        // E3-B4: the page opens with the default-execution-mode and
        // session-title sections (the web `ai.session-memory` composition)
        // and keeps the three memory sections below, advanced collapsed.
        (
            settings::SettingsEntry::Memory,
            &[
                "settings-card-memory-harness",
                "settings-card-memory-session-title",
                "settings-card-memory-basic",
                "settings-card-memory-models",
                "settings-card-memory-advanced",
            ],
        ),
        // The models page is on the card path: the acquisition, selection
        // mode, all-models, and more-settings cards carry the anchors now
        // (the stream and proxy sections live behind the more disclosure).
        (
            settings::SettingsEntry::Models,
            &[
                "settings-card-models-acquisition",
                "settings-card-models-selection-modes",
                "settings-card-models-pool",
                "settings-card-models-more",
            ],
        ),
        // The permissions page is on the card path: one card per web
        // section carries the anchors now.
        (
            settings::SettingsEntry::Permissions,
            &[
                "settings-card-permissions-policy",
                "settings-card-permissions-deferred",
                "settings-card-permissions-tool-presentation",
                "settings-card-permissions-execution",
                "settings-card-permissions-subagents",
                "settings-card-permissions-json-repair",
                "settings-card-permissions-review",
            ],
        ),
        // The MCP page is on the card path: the server-list section is
        // one card with the collection rows under it.
        (settings::SettingsEntry::Mcp, &["settings-card-manage-mcp"]),
        // The automation page is on the card path: the hooks activation
        // switches, the two quick-action collections, the declaration
        // locations, the imported-hook manager, and the Codex reference.
        (
            settings::SettingsEntry::Automation,
            &[
                "settings-card-automation-hooks",
                "settings-card-automation-qa-builtin",
                "settings-card-automation-qa-custom",
                "settings-card-automation-hook-locations",
                "settings-card-automation-hook-imports",
                "settings-card-automation-hooks-compat",
            ],
        ),
        // The skills page is on the card path: the facet rail sits beside
        // the one list card, and the card carries the anchor now.
        (
            settings::SettingsEntry::Skills,
            &["settings-card-manage-skills"],
        ),
        // The agents page is on the card path: the harness overview and
        // the two collection lists carry the anchors now.
        (
            settings::SettingsEntry::Agents,
            &[
                "settings-card-manage-agents-harness",
                "settings-card-manage-agents-modes",
                "settings-card-manage-agents-subagents",
            ],
        ),
        // E3-B5: the ACP page opens on the local registry view, so the
        // registry card carries the anchor; the advanced-JSON card is
        // view-gated and has its own settlement test.
        (
            settings::SettingsEntry::ExternalAgents,
            &["settings-card-acp-registry"],
        ),
        // The pages below migrated before the anchor cases grew a slot;
        // their legacy `settings-section-*` assertions went away with the
        // component pipeline. Each list holds the page's unconditional
        // cards (conditional cards keep their own settlement tests).
        (
            settings::SettingsEntry::General,
            &[
                "settings-card-general-interface",
                "settings-card-general-startup",
                "settings-card-general-window",
            ],
        ),
        (
            settings::SettingsEntry::Terminal,
            &["settings-card-terminal"],
        ),
        // The search page keeps only its jump card: the two session
        // sections moved to the memory page (web composition, E3-B4).
        (
            settings::SettingsEntry::SessionSearch,
            &["settings-card-session-search-open"],
        ),
        (
            settings::SettingsEntry::WorktreesGit,
            &[
                "settings-card-worktrees-search",
                "settings-card-worktrees-attribution",
                "settings-card-worktrees-isolation",
                "settings-card-worktrees-management",
            ],
        ),
        (
            settings::SettingsEntry::WebSearch,
            &["settings-card-websearch"],
        ),
        (
            settings::SettingsEntry::Usage,
            &[
                "settings-card-usage-activity",
                "settings-card-usage-overview",
                "settings-card-usage-distributions",
                "settings-card-usage-details",
            ],
        ),
        (
            settings::SettingsEntry::Archived,
            &["settings-card-archived"],
        ),
        (
            settings::SettingsEntry::Diagnostics,
            &["settings-card-diagnostics"],
        ),
    ];
    for (page, sections) in cases {
        let window = cx.add_empty_window();
        let text = text.clone();
        let view = shell_view.clone();
        let page = *page;
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                cx.new(|_| SettingsHarness {
                    text: text.clone(),
                    view: view.clone(),
                    page,
                })
                .into_any_element()
            },
        );
        window.run_until_parked();
        for selector in *sections {
            assert!(
                window.debug_bounds(selector).is_some(),
                "{page:?} renders {selector}"
            );
        }
    }
}

/// The settings scene owns the nav column: the tree lists the registry
/// pages, a row click selects one, and a query swaps the tree for the
/// matching results (web `SettingsNav` in `NavPanel` + its search mode).
#[gpui_kit::test]
fn settings_scene_nav_selects_pages_and_searches(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let presets = theme::load_presets();
        let active = presets
            .iter()
            .position(|preset| preset.id == DEFAULT_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active) {
            preset.apply(cx);
        }
        cx.set_global(ShellState {
            presets,
            active_preset: active,
        });
        cx.set_global(settings::ShellConfig::startup_default(LocaleId::ZhCN));
        cx.set_global(sample_shell_text());
    });
    let (ttx, trx) = mpsc::unbounded_channel();
    let mut window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(ttx, trx, window, cx)));
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.tabs.open(TabKind::Scene(SceneId::Settings));
            cx.notify();
        });
    });
    draw_shell_for_test(&mut window, &shell);

    // The tree covers the registry: the page rows are the settings nav's,
    // and the page renders in the web's bounded, centered column.
    assert!(
        window.debug_bounds("settings-nav-page-general").is_some(),
        "the settings nav lists the pages"
    );
    assert_eq!(
        window.update(|_, cx| shell.read(cx).settings_page),
        settings::SettingsEntry::General
    );
    let column = window.debug_bounds("settings-column").expect("column");
    assert!(
        column.size.width <= px(SETTINGS_CONTENT_WIDTH) + px(1.),
        "the page renders in the bounded column, got {:?}",
        column.size
    );

    // The nav entry point selects the page (the click itself is exercised
    // in the Xvfb smoke: this harness does not route pointer events into
    // the component library's list rows).
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.set_settings_page(settings::SettingsEntry::Appearance, cx)
        });
    });
    draw_shell_for_test(&mut window, &shell);
    assert_eq!(
        window.update(|_, cx| shell.read(cx).settings_page),
        settings::SettingsEntry::Appearance
    );
    assert!(
        window
            .debug_bounds("settings-nav-page-appearance")
            .is_some(),
        "the nav keeps its rows after a page switch"
    );

    // A query replaces the tree with the matching pages.
    set_settings_search_for_test(&mut window, &shell, "mcp");
    assert!(
        window.debug_bounds("settings-nav-page-general").is_none(),
        "the tree folds away while searching"
    );
    assert!(
        window.debug_bounds("settings-nav-result-mcp").is_some(),
        "the matching page is a result row"
    );

    // No match states it, rather than rendering an empty list.
    set_settings_search_for_test(&mut window, &shell, "no-such-setting");
    assert!(window.debug_bounds("settings-nav-empty").is_some());
}

/// Draws the shell as the window root and settles the frame — the test
/// harness paints one frame per `draw` call, so state changes have to be
/// followed by another one before `debug_bounds` sees them.
fn draw_shell_for_test(window: &mut gpui_kit::VisualTestContext, shell: &Entity<ShellWindow>) {
    let shell = shell.clone();
    window.draw(
        point(px(0.), px(0.)),
        size(px(1200.), px(800.)),
        move |_, _| shell.into_any_element(),
    );
    window.run_until_parked();
}

/// Paints one real frame of the window (the root view pipeline, which the
/// harness's ad-hoc `draw` skips): tooltips and other deferred overlays
/// only prepaint here.
fn draw_window_for_test(window: &mut gpui_kit::VisualTestContext) {
    window.update(|window, cx| window.draw(cx).clear(cx));
    window.run_until_parked();
}

/// Writes a query into the settings nav field through the shell's own input
/// (the same path a keystroke takes) and paints the frame it produces.
fn set_settings_search_for_test(
    window: &mut gpui_kit::VisualTestContext,
    shell: &Entity<ShellWindow>,
    query: &str,
) {
    let input = window.update(|_, cx| shell.read(cx).settings_search_input.clone());
    let query = query.to_string();
    window.update(|window, cx| {
        input.update(cx, |state, cx| state.set_value(query, window, cx));
    });
    draw_shell_for_test(window, shell);
}

/// The models page must render the configured catalog (including disabled
/// entries) without a live runtime.
#[gpui_kit::test]
fn settings_models_page_renders_the_catalog(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let presets = theme::load_presets();
        let active = presets
            .iter()
            .position(|preset| preset.id == DEFAULT_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active) {
            preset.apply(cx);
        }
        cx.set_global(ShellState {
            presets,
            active_preset: active,
        });
        cx.set_global(settings::ShellConfig::startup_default(LocaleId::ZhCN));
        cx.set_global(crate::runtime::ModelCatalog {
            loaded: true,
            models: vec![
                crate::runtime::ModelChoice {
                    id: "model-a".into(),
                    name: "Model A".into(),
                    provider: "Provider X".into(),
                    enabled: true,
                },
                crate::runtime::ModelChoice {
                    id: "model-b".into(),
                    name: "Model B".into(),
                    provider: "Provider Y".into(),
                    enabled: false,
                },
            ],
        });
        cx.set_global(sample_shell_text());
    });
    let window = cx.add_empty_window();
    let text = sample_shell_text();
    // A real shell backs the weak handle the profile rows capture; the
    // harness layout never upgrades it.
    let (ttx, trx) = mpsc::unbounded_channel();
    let shell_view = window.update(|window, cx| {
        cx.new(|cx| ShellWindow::new(ttx, trx, window, cx))
            .downgrade()
    });
    window.draw(
        point(px(0.), px(0.)),
        size(px(900.), px(600.)),
        move |_, cx| {
            cx.new(|_| SettingsHarness {
                text: text.clone(),
                view: shell_view.clone(),
                page: settings::SettingsEntry::General,
            })
            .into_any_element()
        },
    );
    window.run_until_parked();
}

struct SettingsHarness {
    text: ShellText,
    /// Which page the harness renders (the nav selects one at a time).
    page: settings::SettingsEntry,
    /// The settings page opens profile dialogs through the shell weak
    /// handle (D57); the harness never upgrades it, a shell-shaped
    /// placeholder is enough for layout.
    view: gpui_kit::WeakEntity<ShellWindow>,
}

impl Render for SettingsHarness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        settings::render_page(&self.text, self.view.clone(), self.page, cx)
    }
}

/// The dock split must hand the body a real box whether or not the terminal
/// is docked: a `ResizablePanelGroup` carries `size_full()` internally and
/// collapses inside a parent whose height is only flex-derived, which is why
/// the split lives in its own sized frame (same recipe as gpui-base's own
/// dock area).
#[gpui_kit::test]
fn dock_split_keeps_the_body_sized(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let presets = theme::load_presets();
        let active = presets
            .iter()
            .position(|preset| preset.id == DEFAULT_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active) {
            preset.apply(cx);
        }
        cx.set_global(ShellState {
            presets,
            active_preset: active,
        });
        // This test's subject is the bottom dock's body sizing; the shared
        // preference's fresh-install default is `right`, so pin the dock
        // the assertions describe.
        let mut config = settings::ShellConfig::startup_default(LocaleId::ZhCN);
        config.terminal.panel_position = "bottom".to_string();
        cx.set_global(config);
        cx.set_global(sample_shell_text());
    });
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
        let shell = shell.clone();
        move |_, _| shell.into_any_element()
    });
    window.run_until_parked();
    let body = window
        .debug_bounds("shell-body-sidebar")
        .unwrap_or_else(|| {
            panic!(
                "body lays out with the terminal closed: frame {:?}",
                window.debug_bounds("shell-dock-frame")
            )
        });
    assert!(body.size.height > px(300.), "body height: {:?}", body.size);
    assert!(body.size.width > px(200.), "body width: {:?}", body.size);
    assert!(window.debug_bounds("shell-terminal-dock").is_none());

    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.terminal_open = true;
            cx.notify();
        })
    });
    window.draw(
        point(px(0.), px(0.)),
        size(px(1200.), px(800.)),
        move |_, _| shell.into_any_element(),
    );
    window.run_until_parked();
    let docked = window
        .debug_bounds("shell-terminal-dock")
        .expect("terminal dock lays out once opened");
    assert_eq!(docked.size.height, TerminalPane::panel_height());
    let body = window
        .debug_bounds("shell-body-sidebar")
        .expect("body keeps its box with the terminal docked");
    assert!(body.size.height > px(300.), "body height: {:?}", body.size);
}

/// The global status bar (R1d) lays out as a full-width strip pinned to
/// the window bottom, under the body.
#[gpui_kit::test]
fn status_bar_lays_out_at_the_window_bottom(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let presets = theme::load_presets();
        let active = presets
            .iter()
            .position(|preset| preset.id == DEFAULT_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active) {
            preset.apply(cx);
        }
        cx.set_global(ShellState {
            presets,
            active_preset: active,
        });
        cx.set_global(settings::ShellConfig::startup_default(LocaleId::ZhCN));
        cx.set_global(sample_shell_text());
    });
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
        let shell = shell.clone();
        move |_, _| shell.into_any_element()
    });
    window.run_until_parked();
    // The desktop removed the full-width window-bottom status bar
    // (`92ce04f8d`): the user cluster and the tool buttons live in
    // the nav column's footer now — column-wide, hugging the window
    // bottom under the sidebar — while the scene face's top bar carries
    // the tab strip and fills the rest of the window width.
    let nav = window
        .debug_bounds("shell-nav-column")
        .expect("nav column lays out");
    let footer = window
        .debug_bounds("shell-sidebar-footer")
        .expect("sidebar footer lays out");
    assert_eq!(
        footer.size.width, nav.size.width,
        "the footer spans the nav column, not the window"
    );
    assert!(
        (footer.bottom() - nav.bottom()).abs() < px(1.),
        "the footer hugs the window bottom inside the nav column: footer {:?} nav {:?}",
        footer,
        nav
    );
    let sidebar = window
        .debug_bounds("shell-body-sidebar")
        .expect("sidebar lays out");
    assert!(
        sidebar.bottom() <= footer.origin.y + px(1.),
        "the footer sits below the sidebar, not beside it"
    );
    let top_bar = window
        .debug_bounds("shell-scene-top-bar")
        .expect("scene top bar lays out");
    assert!(
        (top_bar.size.width + nav.size.width - px(1200.)).abs() < px(1.),
        "the scene top bar fills the width the nav column leaves: bar {:?} nav {:?}",
        top_bar,
        nav
    );
    assert!(
        (top_bar.size.height - px(TOP_BAR_HEIGHT)).abs() < px(1.),
        "the scene top bar keeps the desktop 45px chrome height"
    );
}

/// The permission badge (R2a) projects the stored mode: ask keeps the
/// plain shield on success ink, auto raises the check shield, and full
/// access raises the alert shield on danger ink — the web icon ramp, one
/// step per given-up confirmation.
#[test]
fn permission_badge_maps_the_settings_preset() {
    let text = gpui_foundation::i18n::fallback_text(LocaleId::EnUS);
    let (icon, label, full_access, tooltip) = ShellWindow::permission_badge("ask", &text);
    assert_eq!(icon, IconName::Shield);
    assert_eq!(label, text.permission_ask);
    assert!(!full_access);
    assert_eq!(tooltip, text.permission_ask_desc);
    let (icon, label, full_access, tooltip) = ShellWindow::permission_badge("auto", &text);
    assert_eq!(icon, IconName::ShieldCheck);
    assert_eq!(label, text.permission_auto);
    assert!(!full_access);
    assert_eq!(tooltip, text.permission_auto_desc);
    let (icon, label, full_access, tooltip) = ShellWindow::permission_badge("full_access", &text);
    assert_eq!(icon, IconName::ShieldAlert);
    assert_eq!(label, text.permission_full_access);
    assert!(full_access);
    assert_eq!(tooltip, text.permission_full_access_desc);
    // An unknown stored mode fails closed to the ask presentation.
    assert_eq!(
        ShellWindow::permission_badge("future-mode", &text),
        ShellWindow::permission_badge("ask", &text)
    );
}

/// The composer (R2a) carries the context strip above the card and the
/// model picker inside it: the strip reads as the capsule's footnote, and
/// the picker's old header home renders no such selector anymore.
#[gpui_kit::test]
fn composer_strip_and_model_picker_lay_out_in_the_composer(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: Some("model-a".into()),
                    entries: Vec::new(),
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
        });
    });
    window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
        let shell = shell.clone();
        move |_, _| shell.into_any_element()
    });
    window.run_until_parked();
    let strip = window
        .debug_bounds("composer-context-strip")
        .expect("context strip lays out");
    let card = window
        .debug_bounds("composer-card")
        .expect("composer card lays out");
    let badge = window
        .debug_bounds("composer-permission-badge")
        .expect("permission badge lays out");
    let picker = window
        .debug_bounds("composer-model-picker")
        .expect("model picker lays out inside the composer");
    assert!(
        strip.origin.y + strip.size.height <= card.origin.y,
        "the strip sits above the card: strip {:?} card {:?}",
        strip,
        card
    );
    assert!(
        badge.origin.y >= strip.origin.y
            && badge.origin.y + badge.size.height <= strip.origin.y + strip.size.height,
        "the permission badge lives in the strip's right rail: badge {:?} strip {:?}",
        badge,
        strip
    );
    assert!(
        picker.origin.y > strip.origin.y
            && picker.origin.y >= card.origin.y
            && picker.origin.y + picker.size.height <= card.origin.y + card.size.height,
        "the model picker moved into the card below the strip: picker {:?} strip {:?} card {:?}",
        picker,
        strip,
        card
    );
}

/// W-8 ⑤: the stop button's tooltip must not outlive the button. The kit's
/// managed tooltip (`Button::tooltip`) hides only on its trigger's
/// hover-end, and a turn that settles under a resting pointer swaps the
/// button out before that event can arrive, so the popup used to stay on
/// screen (frame `w8-06`); the composer anchors the tooltip on a wrapper
/// that leaves the tree with the button instead. Both halves are pinned
/// here: shown while the stop button is up and hovered, released in the
/// frame the turn settles — without the pointer moving.
#[gpui_kit::test]
fn stop_tooltip_releases_with_the_button_when_the_turn_settles(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    // The shell rides a window root view here, not the harness's ad-hoc
    // `draw`: only a real frame prepaints tooltips, which is the whole
    // point of what this test has to watch.
    let (shell, window) = cx.add_window_view(|window, cx| ShellWindow::new(tx, rx, window, cx));
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: Some("model-a".into()),
                    entries: Vec::new(),
                    busy: true,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
        });
    });
    draw_window_for_test(window);
    let anchor = window
        .debug_bounds("chat-stop-tooltip-anchor")
        .expect("the running turn shows the stop button");
    assert_eq!(
        anchor.size,
        size(px(32.), px(32.)),
        "the tooltip anchor is exactly the icon button's box"
    );

    // The pointer comes to rest on the stop button and the tooltip shows
    // through the same delay the kit's managed tooltip used.
    window.simulate_mouse_move(anchor.center(), None, Modifiers::default());
    draw_window_for_test(window);
    window
        .dispatcher
        .advance_clock(std::time::Duration::from_millis(600));
    window.run_until_parked();
    draw_window_for_test(window);
    assert!(
        window.debug_bounds("chat-stop-tooltip").is_some(),
        "hovering the stop button shows its tooltip"
    );

    // The turn settles under the stationary pointer.
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::TurnFinished {
                    session_id: "s-1".into(),
                    error: None,
                    duration_ms: Some(1_200),
                    cancelled: false,
                },
                window,
                cx,
            );
        });
    });
    draw_window_for_test(window);
    assert!(
        window.debug_bounds("chat-stop-tooltip-anchor").is_none(),
        "the stop button leaves with the settled turn"
    );
    assert!(
        window.debug_bounds("chat-stop-tooltip").is_none(),
        "and its tooltip does not outlive it"
    );
}

/// The tabless landing face (R2b): with no tab open the main area shows
/// the brand slogan over the start-here input card, the web WelcomeScene
/// order — not the empty panel (which stays for the unloaded-tab state).
#[gpui_kit::test]
fn welcome_face_lays_out_when_no_tab_is_active(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
        let shell = shell.clone();
        move |_, _| shell.into_any_element()
    });
    window.run_until_parked();
    let face = window
        .debug_bounds("welcome-face")
        .expect("the welcome face lays out with no tabs open");
    let brand = window
        .debug_bounds("welcome-brand")
        .expect("the brand slogan lays out");
    let card = window
        .debug_bounds("welcome-input-card")
        .expect("the start-here input card lays out");
    assert!(
        brand.origin.y >= face.origin.y
            && brand.origin.y + brand.size.height <= face.origin.y + face.size.height,
        "the brand lives inside the face: brand {:?} face {:?}",
        brand,
        face
    );
    assert!(
        card.origin.y >= face.origin.y
            && card.origin.y + card.size.height <= face.origin.y + face.size.height,
        "the input card lives inside the face: card {:?} face {:?}",
        card,
        face
    );
    assert!(
        brand.origin.y + brand.size.height <= card.origin.y,
        "the brand stands above the input card: brand {:?} card {:?}",
        brand,
        card
    );
}

/// The changes view must render its failure state (no runtime attached in
/// tests) instead of panicking on the missing git port.
#[gpui_kit::test]
fn git_panel_renders_without_a_runtime(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(sample_shell_text());
    });
    let window = cx.add_empty_window();
    let panel = window.update(|window, cx| cx.new(|cx| GitPanel::new(None, None, window, cx)));
    window.draw(
        point(px(0.), px(0.)),
        size(px(900.), px(600.)),
        move |_, _| panel.into_any_element(),
    );
    window.run_until_parked();
}

/// G-G1: the git panel's init hand-off stages composer text, and the next
/// render moves it into the composer input (render is the only place that
/// holds the window `set_value` needs).
#[gpui_kit::test]
fn prefill_composer_stages_the_draft_for_the_next_paint(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.prefill_composer("git init please".into(), cx)
        });
    });
    window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
        let shell = shell.clone();
        move |_, _| shell.into_any_element()
    });
    window.run_until_parked();

    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            assert!(
                shell.pending_composer_draft.is_none(),
                "the render path consumes the staged draft"
            );
            assert_eq!(shell.draft.read(cx).value(), "git init please");
        });
    });
}

/// The M11 session-management events must project onto both surfaces: a
/// rename re-labels the row and the open header, a delete closes the pane,
/// and a failure surfaces instead of disappearing.
#[gpui_kit::test]
fn session_management_events_project_onto_the_shell(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::Sessions(vec![
                    crate::runtime::SessionListRow {
                        summary: sample_summary("s-1", "First"),
                        archived: false,
                    },
                    crate::runtime::SessionListRow {
                        summary: sample_summary("s-2", "Second"),
                        archived: true,
                    },
                ]),
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: Some("model-a".into()),
                    entries: Vec::new(),
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
        });
    });

    let (rows, open_name, model) = window.update(|_, cx| {
        let shell = shell.read(cx);
        (
            shell.sessions.clone(),
            shell.active_chat().map(|chat| chat.name.clone()),
            shell.active_chat().and_then(|chat| chat.model_id.clone()),
        )
    });
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row.archived));
    assert_eq!(open_name.as_deref(), Some("First"));
    assert_eq!(model.as_deref(), Some("model-a"));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionRenamed {
                    session_id: "s-1".into(),
                    name: "Renamed".into(),
                },
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::SessionModelChanged {
                    session_id: "s-1".into(),
                    model_id: "model-b".into(),
                },
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::SessionActionFailed {
                    message: "delete: boom".into(),
                },
                window,
                cx,
            );
        });
    });

    let (row_name, chat_name, model, chat_error) = window.update(|_, cx| {
        let shell = shell.read(cx);
        (
            shell
                .sessions
                .iter()
                .find(|row| row.id == "s-1")
                .map(|row| row.name.clone()),
            shell.active_chat().map(|chat| chat.name.clone()),
            shell.active_chat().and_then(|chat| chat.model_id.clone()),
            shell.chat_error.clone(),
        )
    });
    assert_eq!(row_name.as_deref(), Some("Renamed"));
    assert_eq!(chat_name.as_deref(), Some("Renamed"));
    assert_eq!(model.as_deref(), Some("model-b"));
    assert!(chat_error.is_some(), "a failed action must surface");

    // Deleting the open session closes the pane and drops the row.
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionDeleted {
                    session_id: "s-1".into(),
                },
                window,
                cx,
            );
        });
    });
    let (rows, chat_open) = window.update(|_, cx| {
        let shell = shell.read(cx);
        (shell.sessions.len(), !shell.chats.is_empty())
    });
    assert_eq!(rows, 1);
    assert!(!chat_open);
}

/// The turn echo stamps the fork boundary onto the user entry, and the
/// compaction/export/usage events feed their dialogs and status lines.
#[gpui_kit::test]
fn turn_echo_and_maintenance_events_update_chat_state(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: Vec::new(),
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::TurnAccepted {
                    session_id: "s-1".into(),
                    turn_id: "turn-9".into(),
                    message: "do the thing".into(),
                },
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::CompactionStarted {
                    session_id: "s-1".into(),
                },
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::ExportFinished {
                    session_id: "s-1".into(),
                    path: "/tmp/export.md".into(),
                },
                window,
                cx,
            );
        });
    });

    window.update(|_, cx| {
        let shell = shell.read(cx);
        let chat = shell.active_chat().expect("chat opened");
        assert!(chat.busy, "compaction marks the session busy");
        let mut entries = chat.entries.iter();
        match entries.next_back() {
            Some(ChatEntry::Info { text }) => assert!(text.contains("/tmp/export.md")),
            other => panic!("expected export info line, got {other:?}"),
        }
        match entries.next_back() {
            Some(ChatEntry::Info { text }) => assert!(!text.is_empty()),
            other => panic!("expected compaction info line, got {other:?}"),
        }
        match entries.next_back() {
            Some(ChatEntry::User { turn_id, .. }) => {
                assert_eq!(turn_id.as_deref(), Some("turn-9"));
            }
            other => panic!("expected user entry with turn id, got {other:?}"),
        }
        assert!(shell.usage_report.is_none(), "usage dialog stays closed");
    });
}

#[gpui_kit::test]
fn send_failure_keeps_the_prompt_for_continue_editing(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: Vec::new(),
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
            // A failed submission keeps the prompt visible as the failed
            // preview instead of dropping it with the draft.
            shell.apply_runtime_event(
                RuntimeEvent::SendFailed {
                    session_id: "s-1".into(),
                    message: Some("retry me".into()),
                    error: "offline".into(),
                },
                window,
                cx,
            );
            // A /btw failure carries no prompt and stays notice-only.
            shell.apply_runtime_event(
                RuntimeEvent::SendFailed {
                    session_id: "s-1".into(),
                    message: None,
                    error: "coordinator not ready".into(),
                },
                window,
                cx,
            );
            // The next accepted turn echoes behind the failed preview.
            shell.apply_runtime_event(
                RuntimeEvent::TurnAccepted {
                    session_id: "s-1".into(),
                    turn_id: "turn-1".into(),
                    message: "retry me".into(),
                },
                window,
                cx,
            );
        });
    });

    window.update(|_, cx| {
        let shell = shell.read(cx);
        let chat = shell.active_chat().expect("chat opened");
        assert!(!chat.submitting, "the failed submission unblocks sending");
        let mut entries = chat.entries.iter();
        match entries.next_back() {
            Some(ChatEntry::User { text, .. }) => assert_eq!(text, "retry me"),
            other => panic!("expected the accepted echo, got {other:?}"),
        }
        match entries.next_back() {
            // The notice text resolves through the i18n service, which
            // tests run without: presence is the contract here.
            Some(ChatEntry::Notice { .. }) => {}
            other => panic!("expected the btw error notice, got {other:?}"),
        }
        match entries.next_back() {
            Some(ChatEntry::FailedUser { text, error }) => {
                assert_eq!(text, "retry me");
                assert_eq!(error, "offline");
            }
            other => panic!("expected the failed preview, got {other:?}"),
        }
    });
}

#[gpui_kit::test]
fn transcript_renders_the_failed_submission_entry(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| gpui_kit::init(cx));

    let entries = Rc::new(vec![ChatEntry::FailedUser {
        text: "hello".into(),
        error: "offline".into(),
    }]);
    let render_count: Rc<Cell<usize>> = Default::default();
    let scroller = cx.new(|cx| MessageScrollerState::new(entries.len(), cx));
    let window = cx.add_empty_window();
    let mut draw = |render_count: Rc<Cell<usize>>| {
        let scroller = scroller.clone();
        let entries = entries.clone();
        render_count.set(0);
        window.draw(
            point(px(0.), px(0.)),
            size(px(800.), px(600.)),
            move |_, cx| {
                cx.new(|_| TranscriptListHarness {
                    scroller,
                    entries,
                    render_count,
                    text: sample_shell_text(),
                })
                .into_any_element()
            },
        );
    };
    // The failed preview renders like any row (no fork context here, so
    // the refill button stays off); two draws settle the first layout.
    draw(render_count.clone());
    draw(render_count.clone());
    assert!(
        render_count.get() >= 1,
        "the failed preview row must render"
    );
}

#[test]
fn usage_summary_formats_tokens_and_durations() {
    assert_eq!(optional_tokens(Some(120)), "120");
    assert_eq!(optional_tokens(None), "—");
    assert_eq!(optional_duration(Some(1_500)), "1.5 s");
    assert_eq!(optional_duration(Some(61_000)), "1 m 1 s");
    assert_eq!(optional_duration(None), "—");
}

/// Insights and scheduled-jobs events project onto their page state
/// without touching the chat surface.
#[gpui_kit::test]
fn insights_and_tasks_events_project_onto_page_state(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(RuntimeEvent::InsightsGenerating, window, cx);
            assert!(shell.insights.generating);
            shell.apply_runtime_event(
                RuntimeEvent::InsightsFailed("insights: boom".into()),
                window,
                cx,
            );
            assert!(!shell.insights.generating);
            assert_eq!(shell.insights.error.as_deref(), Some("insights: boom"));
            shell.apply_runtime_event(RuntimeEvent::CronJobs(vec![]), window, cx);
            shell.apply_runtime_event(RuntimeEvent::CronFailed("cron: boom".into()), window, cx);
        });
    });

    window.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(shell.tasks.loaded);
        assert_eq!(shell.tasks.error.as_deref(), Some("cron: boom"));
        assert!(shell.chats.is_empty(), "page events never touch the chat");
    });
}

/// The insights page keeps a scroll extent for long content: the page
/// column must grow past the viewport so the report body and the
/// suggestions below the fold stay reachable. A column pinned to
/// `h_full()` capped the scrollable content at the container height (found
/// by the G-I5/G-I6 visual pass against a real report).
#[gpui_kit::test]
fn insights_page_keeps_a_scroll_extent_for_long_content(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|_window, cx| {
        shell.update(cx, |shell, cx| {
            shell.tabs.open(TabKind::Scene(tabs::SceneId::Insights));
            shell.insights.loaded = true;
            shell.insights.latest = (0..20)
                .map(
                    |index| openbitfun_core::agentic::insights::InsightsReportMeta {
                        generated_at: 1_785_725_436 + index,
                        total_sessions: 6,
                        analyzed_sessions: 6,
                        date_range: openbitfun_core::agentic::insights::DateRange {
                            start: "2026-08-01T16:31:29.115+00:00".into(),
                            end: "2026-08-03T00:44:44.204+00:00".into(),
                        },
                        path: format!("/tmp/usage-data/insights-1785725436-{index}.json"),
                        total_messages: 243,
                        days_covered: 3,
                        total_hours: 1.2,
                        top_goals: vec!["goals".into()],
                        languages: vec!["Rust".into()],
                        session_usage: Default::default(),
                        generation_usage: Default::default(),
                        generation_models: vec!["deepseek-v4-flash-free".into()],
                    },
                )
                .collect();
            cx.notify();
        });
    });

    let shell_for_draw = shell.clone();
    window.draw(
        point(px(0.), px(0.)),
        size(px(1200.), px(800.)),
        move |_, _| shell_for_draw.into_any_element(),
    );
    window.run_until_parked();

    let container = window
        .debug_bounds("insights-page")
        .expect("the insights page renders inside its scroll container");
    let content = window
        .debug_bounds("insights-page-content")
        .expect("the page column renders");
    assert!(
        content.size.height > container.size.height,
        "long content keeps a scroll extent: content height {} vs container height {}",
        content.size.height,
        container.size.height
    );
}

/// The tasks page shares the insights page's wrapper + column shape: its
/// column must keep a scroll extent for long content (the job list grows
/// without bound).
#[gpui_kit::test]
fn tasks_page_keeps_a_scroll_extent_for_long_content(cx: &mut gpui_kit::TestAppContext) {
    use openbitfun_core::service::cron::{
        CronJob, CronJobPayload, CronJobTarget, CronSchedule, CronWorkspaceRef,
    };

    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let mut window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    let draw_page = |window: &mut gpui_kit::VisualTestContext, shell: &Entity<ShellWindow>| {
        let shell = shell.clone();
        window.draw(
            point(px(0.), px(0.)),
            size(px(1200.), px(800.)),
            move |_, _| shell.into_any_element(),
        );
        window.run_until_parked();
    };

    // Tasks: a long all-jobs list.
    window.update(|_window, cx| {
        shell.update(cx, |shell, cx| {
            shell.tabs.open(TabKind::Scene(tabs::SceneId::Tasks));
            shell.tasks.loaded = true;
            shell.tasks.jobs = (0..30)
                .map(|index| CronJob {
                    report_policy: Default::default(),
                    id: format!("job-{index}"),
                    name: format!("job-{index}"),
                    schedule: CronSchedule::Cron {
                        expr: "0 9 * * *".to_string(),
                        tz: None,
                    },
                    payload: CronJobPayload {
                        text: String::new(),
                    },
                    enabled: true,
                    target: CronJobTarget::Workspace {
                        workspace: CronWorkspaceRef {
                            workspace_id: None,
                            workspace_path: String::new(),
                            project_workspace_path: None,
                            execution_target: None,
                            remote_connection_id: None,
                            remote_ssh_host: None,
                        },
                        launch: Default::default(),
                    },
                    created_at_ms: 0,
                    config_updated_at_ms: 0,
                    updated_at_ms: 0,
                    state: Default::default(),
                })
                .collect();
            cx.notify();
        });
    });
    draw_page(&mut window, &shell);
    let container = window
        .debug_bounds("tasks-page")
        .expect("the tasks page renders inside its scroll container");
    let content = window
        .debug_bounds("tasks-page-content")
        .expect("the tasks page column renders");
    assert!(
        content.size.height > container.size.height,
        "the tasks page keeps a scroll extent: content height {} vs container height {}",
        content.size.height,
        container.size.height
    );
}

/// G-P1: the scheduled-job dialog opens over the shell window (dialogs are
/// window overlays, so this needs a real root-view frame) and prefills from
/// the edited job through the window-owned inputs the save path reads.
#[gpui_kit::test]
fn task_editor_opens_prefilled_from_the_edited_job(cx: &mut gpui_kit::TestAppContext) {
    use openbitfun_core::service::cron::{
        CronJob, CronJobPayload, CronJobTarget, CronSchedule, CronWorkspaceRef,
    };

    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let editor_tx = tx.clone();
    // Dialogs live on the gpui-component window root, so the test window
    // needs the same `Root` wrapper the real `gpui_kit::open_window`
    // installs; a bare view would panic at `WindowState`.
    let (root, window) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| ShellWindow::new(tx, rx, window, cx));
        gpui_kit::base::Root::new(shell, window, cx)
    });
    let shell = window.update(|_, cx| {
        root.read(cx)
            .view()
            .clone()
            .downcast::<ShellWindow>()
            .ok()
            .expect("the window root wraps the shell")
    });
    let job = CronJob {
        report_policy: Default::default(),
        id: "job-edit".to_string(),
        name: "Nightly".to_string(),
        schedule: CronSchedule::Cron {
            expr: "0 9 * * *".to_string(),
            tz: Some("Asia/Shanghai".to_string()),
        },
        payload: CronJobPayload {
            text: "Summarize the day".to_string(),
        },
        enabled: false,
        target: CronJobTarget::Workspace {
            workspace: CronWorkspaceRef {
                workspace_id: Some("ws-1".to_string()),
                workspace_path: "/tmp/ws".to_string(),
                project_workspace_path: None,
                execution_target: None,
                remote_connection_id: None,
                remote_ssh_host: None,
            },
            launch: Default::default(),
        },
        created_at_ms: 0,
        config_updated_at_ms: 0,
        updated_at_ms: 0,
        state: Default::default(),
    };
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.tabs.open(TabKind::Scene(tabs::SceneId::Tasks));
            shell.tasks.loaded = true;
            cx.notify();
        });
        crate::tasks::open_task_editor(
            shell.downgrade(),
            Some(job.clone()),
            editor_tx.clone(),
            window,
            cx,
        );
    });
    draw_window_for_test(window);
    let editor = window
        .debug_bounds("task-editor")
        .expect("the task editor dialog renders over the window");
    assert!(editor.size.width >= px(500.));

    window.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.task_name_input.read(cx).value().to_string(),
            "Nightly"
        );
        assert_eq!(
            shell.task_prompt_input.read(cx).value().to_string(),
            "Summarize the day"
        );
        assert_eq!(shell.task_editor_kind, crate::tasks::TaskScheduleKind::Cron);
        assert_eq!(
            shell.task_cron_input.read(cx).value().to_string(),
            "0 9 * * *"
        );
        assert_eq!(
            shell.task_tz_input.read(cx).value().to_string(),
            "Asia/Shanghai"
        );
        assert_eq!(shell.task_editor_target.as_deref(), Some("job-edit"));
        assert!(!shell.task_editor_enabled);
    });
}

/// Archived rows stay out of the sidebar until the toggle is on, and the
/// toggle itself never invents rows the runtime did not report.
#[gpui_kit::test]
fn archived_rows_hide_behind_the_toggle(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let mut window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::Sessions(vec![
                    crate::runtime::SessionListRow {
                        summary: sample_summary("s-1", "Active"),
                        archived: false,
                    },
                    crate::runtime::SessionListRow {
                        summary: sample_summary("s-2", "Archived"),
                        archived: true,
                    },
                ]),
                window,
                cx,
            );
        });
    });

    let count = visible_session_rows(&mut window, &shell, &text);
    assert_eq!(count, 1);
    window.update(|_, cx| {
        shell.update(cx, |shell, _cx| {
            shell.show_archived = true;
        });
    });
    let count = visible_session_rows(&mut window, &shell, &text);
    assert_eq!(count, 2);
}

/// The sidebar tree shape: `(session id, depth, running)` per session row
/// in render order, the fold row as `Err((remaining, expanded))`.
/// Flattens the built sidebar sections into the shape assertions use:
/// one `Ok((id, depth, running))` per session row and one
/// `Err((remaining, expanded))` per show-more row, in render order. Rows
/// live inside their workspace group's single child, so the walk goes
/// through [`SidebarSection::Rows`].
fn session_tree_shape(
    window: &mut gpui_kit::VisualTestContext,
    shell: &Entity<ShellWindow>,
    text: &ShellText,
) -> Vec<Result<(String, u8, bool), (usize, bool)>> {
    let text = text.clone();
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let mut shape = Vec::new();
            for section in shell.render_session_entries(&text, cx) {
                let entries = match section {
                    SidebarSection::Rows(entries) => entries,
                    SidebarSection::Item(entry) => vec![entry],
                };
                for entry in entries {
                    match entry {
                        SidebarEntry::Session {
                            row,
                            depth,
                            running,
                            ..
                        } => shape.push(Ok((row.id, depth, running))),
                        SidebarEntry::More {
                            remaining,
                            expanded,
                            ..
                        } => shape.push(Err((remaining, expanded))),
                        _ => {}
                    }
                }
            }
            shape
        })
    })
}

fn lineage_entry(
    session_id: &str,
    session_name: &str,
    parent_session_id: Option<&str>,
    subagent_type: Option<&str>,
) -> openbitfun_runtime_ports::AgentSessionLineageEntry {
    openbitfun_runtime_ports::AgentSessionLineageEntry {
        session_id: session_id.into(),
        session_name: session_name.into(),
        agent_type: "Standard".into(),
        created_at_ms: 0,
        status: openbitfun_runtime_ports::AgentSessionLifecycleStatus::Active,
        active_turn_id: None,
        parent_session_id: parent_session_id.map(str::to_string),
        parent_tool_call_id: None,
        subagent_type: subagent_type.map(str::to_string),
        agent_id: None,
        workspace_id: None,
        workspace_path: None,
        remote_connection_id: None,
        remote_ssh_host: None,
        unread_completion: None,
        needs_user_attention: None,
    }
}

/// R1c: lineage snapshots project as subagent children indented under
/// their root, one level deep — deeper descendants stay hidden like the
/// web nav's two-level projection.
#[gpui_kit::test]
fn session_tree_projects_children_under_roots(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let mut window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::Sessions(vec![
                    crate::runtime::SessionListRow {
                        summary: sample_summary("s-1", "Root one"),
                        archived: false,
                    },
                    crate::runtime::SessionListRow {
                        summary: sample_summary("s-2", "Root two"),
                        archived: false,
                    },
                ]),
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::SessionLineages(vec![
                    openbitfun_runtime_ports::AgentSessionLineageSnapshot {
                        root_session_id: "s-1".into(),
                        sessions: vec![
                            lineage_entry("s-1", "Root one", None, None),
                            lineage_entry("c-1", "Child", Some("s-1"), Some("Explore")),
                            lineage_entry("g-1", "Grandchild", Some("c-1"), None),
                        ],
                    },
                    openbitfun_runtime_ports::AgentSessionLineageSnapshot {
                        root_session_id: "s-2".into(),
                        sessions: vec![lineage_entry("s-2", "Root two", None, None)],
                    },
                ]),
                window,
                cx,
            );
        });
    });

    let shape = session_tree_shape(&mut window, &shell, &text);
    assert_eq!(
        shape,
        vec![
            Ok(("s-1".into(), 0, false)),
            Ok(("c-1".into(), 1, false)),
            Ok(("s-2".into(), 0, false)),
        ]
    );
}

/// R1c: long root lists fold to the collapsed page with a show-more row,
/// expand back to every root, and the workspace group folds everything
/// but its own row.
#[gpui_kit::test]
fn session_tree_folds_long_lists(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let mut window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let rows = (0..8)
                .map(|ix| crate::runtime::SessionListRow {
                    summary: sample_summary(&format!("s-{ix}"), &format!("Session {ix}")),
                    archived: false,
                })
                .collect();
            shell.apply_runtime_event(RuntimeEvent::Sessions(rows), window, cx);
        });
    });

    let shape = session_tree_shape(&mut window, &shell, &text);
    assert_eq!(shape.iter().filter(|row| row.is_ok()).count(), 5);
    assert_eq!(shape.last(), Some(&Err((3, false))));

    window.update(|_, cx| {
        shell.update(cx, |shell, _cx| {
            shell.sessions_expanded = true;
        });
    });
    let shape = session_tree_shape(&mut window, &shell, &text);
    assert_eq!(shape.iter().filter(|row| row.is_ok()).count(), 8);
    assert_eq!(shape.last(), Some(&Err((0, true))));

    window.update(|_, cx| {
        shell.update(cx, |shell, _cx| {
            shell.sessions_group_collapsed = true;
        });
    });
    let shape = session_tree_shape(&mut window, &shell, &text);
    assert!(shape.is_empty());
}

/// The sidebar's child count must not move when a group folds or a long
/// list expands: `Sidebar` resets its virtualized list (and the scroll
/// offset with it) whenever the child count changes, so one child per
/// workspace group is what keeps a click on a project row from throwing
/// the list back to the top.
#[gpui_kit::test]
fn sidebar_keeps_one_child_per_group_across_folds(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let mut window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let rows = (0..7)
                .map(|ix| crate::runtime::SessionListRow {
                    summary: sample_summary(&format!("s-{ix}"), &format!("Session {ix}")),
                    archived: false,
                })
                .collect();
            shell.apply_runtime_event(RuntimeEvent::Sessions(rows), window, cx);
            shell.apply_runtime_event(
                RuntimeEvent::OtherWorkspaceSessions {
                    name: "zenix".into(),
                    root_path: "/home/zero/zenix".into(),
                    rows: vec![crate::runtime::SessionListRow {
                        summary: sample_summary("m-3", "M3"),
                        archived: false,
                    }],
                },
                window,
                cx,
            );
        });
    });

    let children = |window: &mut gpui_kit::VisualTestContext| {
        let text = text.clone();
        window.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .render_session_entries(&text, cx)
                    .into_iter()
                    .map(|section| match section {
                        SidebarSection::Rows(rows) => rows.len(),
                        _ => 1,
                    })
                    .collect::<Vec<_>>()
            })
        })
    };

    // One child per workspace group, and the rows stay inside them: the
    // current group folds its long list (header + 5 rows + show-more +
    // collapse row), zenix shows its single row.
    let before = children(&mut window);
    assert_eq!(before, vec![8, 3], "current group + zenix group");

    window.update(|_, cx| {
        shell.update(cx, |shell, _cx| {
            shell.sessions_group_collapsed = true;
            shell.sessions_expanded = true;
            if let Some(group) = shell.other_groups.first_mut() {
                group.collapsed = true;
            }
        });
    });
    let folded = children(&mut window);
    assert_eq!(
        folded.len(),
        before.len(),
        "folding must not change the number of sidebar children"
    );
    assert_eq!(
        folded,
        vec![1, 1],
        "a folded group keeps its header row only"
    );

    // Row-list traffic stays inside the group too: a session arriving, a
    // new workspace group appearing, and a workspace row reload all leave
    // the child count alone (only a new *group* adds a child).
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.sessions_group_collapsed = false;
            if let Some(group) = shell.other_groups.first_mut() {
                group.collapsed = false;
            }
            let rows = (0..9)
                .map(|ix| crate::runtime::SessionListRow {
                    summary: sample_summary(&format!("s-{ix}"), &format!("Session {ix}")),
                    archived: false,
                })
                .collect();
            shell.apply_runtime_event(RuntimeEvent::Sessions(rows), window, cx);
        });
    });
    let reloaded = children(&mut window);
    assert_eq!(
        reloaded.len(),
        before.len(),
        "a session appearing must not change the number of sidebar children"
    );
}

/// Toggling the flat view or switching a filter must not move the child
/// count either — the flat projection fills the current group's child and
/// leaves every other workspace's slot empty — because a changed count
/// resets the sidebar list's scroll anchor (the same rule group folds
/// follow; the web swaps its whole projection here, the shell cannot).
#[gpui_kit::test]
fn sidebar_child_count_holds_across_view_and_filters(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let mut window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let rows = (0..7)
                .map(|ix| crate::runtime::SessionListRow {
                    summary: sample_summary_at(&format!("s-{ix}"), &format!("Session {ix}"), ix),
                    archived: false,
                })
                .collect();
            shell.apply_runtime_event(RuntimeEvent::Sessions(rows), window, cx);
            shell.apply_runtime_event(
                RuntimeEvent::OtherWorkspaceSessions {
                    name: "zenix".into(),
                    root_path: "/home/zero/zenix".into(),
                    rows: vec![crate::runtime::SessionListRow {
                        summary: sample_summary_at("m-3", "M3", 100),
                        archived: false,
                    }],
                },
                window,
                cx,
            );
        });
    });

    let children = |window: &mut gpui_kit::VisualTestContext| {
        let text = text.clone();
        window.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .render_session_entries(&text, cx)
                    .into_iter()
                    .map(|section| match section {
                        SidebarSection::Rows(rows) => rows.len(),
                        _ => 1,
                    })
                    .collect::<Vec<_>>()
            })
        })
    };
    let grouped = children(&mut window);
    assert_eq!(grouped, vec![8, 3], "current group + zenix group");

    // Flat: every workspace's sessions land in the first child (uncapped:
    // 8 roots, not the folded page of 5), the zenix slot goes empty.
    window.update(|_, cx| {
        shell.update(cx, |shell, _cx| shell.session_list.flat = true);
    });
    let flat = children(&mut window);
    assert_eq!(
        flat.len(),
        grouped.len(),
        "switching the view must not change the number of sidebar children"
    );
    assert_eq!(flat, vec![8, 0]);

    // A filter narrows the rows inside the children, never their count —
    // including the honest no-match line when it hides everything.
    window.update(|_, cx| {
        shell.update(cx, |shell, _cx| {
            shell.session_list.unread_only = true;
            shell.session_list.flat = false;
        });
    });
    let filtered = children(&mut window);
    assert_eq!(filtered.len(), grouped.len());
    assert_eq!(
        filtered,
        vec![3, 3],
        "an empty group shows its no-match line inside its own child"
    );
}

/// The flat projection (web `grouping: all`) orders every workspace's
/// rows by one key, labels each root row with its workspace, and keeps the
/// rows' group so opening a foreign session still switches first.
#[gpui_kit::test]
fn flat_projection_orders_every_workspace_and_scopes_rows(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let _text = sample_shell_text();

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let rows = (0..7)
                .map(|ix| crate::runtime::SessionListRow {
                    summary: sample_summary_at(&format!("s-{ix}"), &format!("Session {ix}"), ix),
                    archived: false,
                })
                .collect();
            shell.apply_runtime_event(RuntimeEvent::Sessions(rows), window, cx);
            shell.apply_runtime_event(
                RuntimeEvent::OtherWorkspaceSessions {
                    name: "zenix".into(),
                    root_path: "/home/zero/zenix".into(),
                    rows: vec![crate::runtime::SessionListRow {
                        summary: sample_summary_at("m-3", "M3", 100),
                        archived: false,
                    }],
                },
                window,
                cx,
            );
            shell.session_list.flat = true;
        });
    });

    let entries =
        window.update(|_, cx| shell.update(cx, |shell, cx| shell.flat_entries("current", cx)));
    let rows: Vec<(String, u8, Option<String>, Option<String>)> = entries
        .iter()
        .filter_map(|entry| match entry {
            SidebarEntry::Session {
                row,
                depth,
                group,
                scope,
                ..
            } => Some((
                row.id.clone(),
                *depth,
                group.clone(),
                scope.as_ref().map(|scope| scope.to_string()),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        rows.len(),
        8,
        "the flat list is not capped at the folded page"
    );
    assert_eq!(
        rows[0],
        (
            "m-3".to_string(),
            0,
            Some("/home/zero/zenix".to_string()),
            Some("zenix".to_string())
        ),
        "the newest row across every workspace leads and keeps its group"
    );
    assert_eq!(
        rows[1].0, "s-6",
        "the current workspace's rows follow in the same global ordering"
    );
    assert!(
        !entries
            .iter()
            .any(|entry| matches!(entry, SidebarEntry::GroupHeader { .. })),
        "the flat projection has no group headers"
    );
}

/// The projection pipeline (web `compareWorkspaceNavSessions` +
/// `matchesWorkspaceSessionView` subset): each ordering is total and
/// stable, and the row predicate hides exactly the archived, non-unread
/// and running rows the shell's filters can derive.
#[test]
fn session_list_pipeline_matches_the_web_subset() {
    let row = |id: &str, name: &str, created: u64, active: u64| SessionRow {
        id: id.into(),
        name: name.into(),
        metadata: String::new(),
        archived: false,
        created_at_ms: created,
        last_active_at_ms: active,
    };
    let ids = |rows: &[SessionRow]| rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();

    let mut rows = vec![
        row("a", "beta", 300, 10),
        row("b", "Alpha", 100, 30),
        row("c", "gamma", 200, 20),
    ];
    rows.sort_by(|left, right| compare_session_rows(left, right, SessionOrdering::Updated));
    assert_eq!(ids(&rows), ["b", "c", "a"]);
    rows.sort_by(|left, right| compare_session_rows(left, right, SessionOrdering::Created));
    assert_eq!(ids(&rows), ["a", "c", "b"]);
    // Case-folded names: Alpha < beta < gamma.
    rows.sort_by(|left, right| compare_session_rows(left, right, SessionOrdering::Name));
    assert_eq!(ids(&rows), ["b", "a", "c"]);

    // Equal activity falls back to created-desc, then to the session id.
    let mut ties = vec![row("z", "z", 50, 100), row("y", "y", 90, 100)];
    ties.sort_by(|left, right| compare_session_rows(left, right, SessionOrdering::Updated));
    assert_eq!(ids(&ties), ["y", "z"], "created-desc breaks the tie");

    let defaults = SessionListFilters::default();
    assert!(session_row_matches(
        SessionRowFacts::default(),
        &defaults,
        false
    ));
    let archived = SessionRowFacts {
        archived: true,
        ..Default::default()
    };
    assert!(!session_row_matches(archived, &defaults, false));
    assert!(session_row_matches(archived, &defaults, true));
    let unread_only = SessionListFilters {
        unread_only: true,
        ..SessionListFilters::default()
    };
    assert!(!session_row_matches(
        SessionRowFacts::default(),
        &unread_only,
        true
    ));
    assert!(session_row_matches(
        SessionRowFacts {
            unread: true,
            ..Default::default()
        },
        &unread_only,
        true
    ));
    let no_running = SessionListFilters {
        hide_running: true,
        ..SessionListFilters::default()
    };
    assert!(!session_row_matches(
        SessionRowFacts {
            running: true,
            ..Default::default()
        },
        &no_running,
        true
    ));
    // Only show/status narrow the list; grouping and ordering do not mark
    // the filter button.
    assert!(!defaults.is_customized());
    assert!(no_running.is_customized());
    assert!(unread_only.is_customized());
}

/// Rows under another workspace's group carry that workspace's root — the
/// click handler needs it to switch before opening — while the active
/// workspace's rows stay ungrouped.
#[gpui_kit::test]
fn foreign_group_rows_carry_their_workspace(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::Sessions(vec![crate::runtime::SessionListRow {
                    summary: sample_summary("s-1", "Home"),
                    archived: false,
                }]),
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::OtherWorkspaceSessions {
                    name: "zenix".into(),
                    root_path: "/home/zero/zenix".into(),
                    rows: vec![crate::runtime::SessionListRow {
                        summary: sample_summary("m-3", "M3"),
                        archived: false,
                    }],
                },
                window,
                cx,
            );
        });
    });

    let grouped: Vec<(String, Option<String>)> = window.update(|_, cx| {
        let text = text.clone();
        shell.update(cx, |shell, cx| {
            shell
                .render_session_entries(&text, cx)
                .into_iter()
                .flat_map(|section| match section {
                    SidebarSection::Rows(rows) => rows,
                    _ => Vec::new(),
                })
                .filter_map(|entry| match entry {
                    SidebarEntry::Session { row, group, .. } => Some((row.id, group)),
                    _ => None,
                })
                .collect()
        })
    });
    assert_eq!(
        grouped,
        vec![
            ("s-1".to_string(), None),
            ("m-3".to_string(), Some("/home/zero/zenix".to_string())),
        ]
    );
}

/// The group plan switches only for a group that is not the active
/// workspace; the active workspace's group (or no group at all) acts
/// straight away.
#[test]
fn workspace_group_plan_switches_foreign_workspaces_only() {
    assert_eq!(
        workspace_group_plan(Some("/home/zero/proj-b"), "/home/zero/proj-a"),
        WorkspaceGroupPlan::SwitchThenAct {
            root: "/home/zero/proj-b".to_string()
        }
    );
    assert_eq!(
        workspace_group_plan(Some("/home/zero/proj-a"), "/home/zero/proj-a"),
        WorkspaceGroupPlan::Act
    );
    assert_eq!(
        workspace_group_plan(None, "/home/zero/proj-a"),
        WorkspaceGroupPlan::Act
    );
}

/// The new-project path joins like the web dialog (`${parent}/${name}`
/// with backslashes normalized) and never leaves doubled separators.
#[test]
fn new_project_path_joins_like_the_web_dialog() {
    assert_eq!(new_project_path("/home/zero", "proj"), "/home/zero/proj");
    assert_eq!(new_project_path("/home/zero/", "proj"), "/home/zero/proj");
    assert_eq!(
        new_project_path(r"C:\Users\zero", "proj"),
        "C:/Users/zero/proj"
    );
    assert_eq!(new_project_path("/home/zero", " proj "), "/home/zero/proj");
}

/// 打开项目 accepts only an existing directory, and the create step makes
/// exactly one directory (the web `createDirectory` contract: a mistyped
/// parent chain is refused instead of invented).
#[test]
fn project_paths_validate_and_create_exactly_one_directory() {
    assert_eq!(check_project_path(""), ProjectPathCheck::Empty);
    assert_eq!(check_project_path("   "), ProjectPathCheck::Empty);
    assert_eq!(
        check_project_path("/definitely/not/here-1234567890"),
        ProjectPathCheck::Missing
    );

    let parent = std::env::temp_dir().join(format!(
        "openbitfun-gpui-project-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).expect("temp parent");
    let target = new_project_path(&parent.to_string_lossy(), "proj");
    assert_eq!(check_project_path(&target), ProjectPathCheck::Missing);
    create_project_dir(&target).expect("create project");
    assert_eq!(check_project_path(&target), ProjectPathCheck::Ok);

    // A second create of the same path fails (the directory is a fact,
    // not an idempotent no-op) and a missing parent is not invented.
    assert!(create_project_dir(&target).is_err());
    let orphan = new_project_path(
        &new_project_path(&parent.to_string_lossy(), "missing-parent"),
        "proj",
    );
    assert!(create_project_dir(&orphan).is_err());
    let _ = std::fs::remove_dir_all(&parent);
}

/// R1c: the running dot follows the process-wide live set, so background
/// sessions light up without being open.
#[gpui_kit::test]
fn session_tree_lights_running_rows(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let mut window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::Sessions(vec![crate::runtime::SessionListRow {
                    summary: sample_summary("s-1", "Background"),
                    archived: false,
                }]),
                window,
                cx,
            );
        });
    });
    let shape = session_tree_shape(&mut window, &shell, &text);
    assert_eq!(shape, vec![Ok(("s-1".into(), 0, false))]);

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::TurnStarted {
                    session_id: "s-1".into(),
                    turn_id: "t-1".into(),
                },
                window,
                cx,
            );
        });
    });
    let shape = session_tree_shape(&mut window, &shell, &text);
    assert_eq!(shape, vec![Ok(("s-1".into(), 0, true))]);

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::TurnFinished {
                    session_id: "s-1".into(),
                    error: None,
                    duration_ms: None,
                    cancelled: false,
                },
                window,
                cx,
            );
        });
    });
    let shape = session_tree_shape(&mut window, &shell, &text);
    assert_eq!(shape, vec![Ok(("s-1".into(), 0, false))]);
}

/// The archived settings page renders its bulk delete affordance next to
/// the rows (T1-4); the destructive step stays behind its confirm dialog,
/// so the render alone must not delete anything.
#[gpui_kit::test]
fn archived_settings_page_renders_the_bulk_delete(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::Sessions(vec![
                    crate::runtime::SessionListRow {
                        summary: sample_summary("s-1", "Active"),
                        archived: false,
                    },
                    crate::runtime::SessionListRow {
                        summary: sample_summary("s-2", "Archived"),
                        archived: true,
                    },
                ]),
                window,
                cx,
            );
            // The page's rows come from the cross-workspace projection, not
            // the session list; seed one group so the row renders.
            shell.apply_runtime_event(
                RuntimeEvent::ArchivedSessions(vec![crate::runtime::ArchivedWorkspaceGroup {
                    workspace_id: "w-1".into(),
                    name: "a5-ws".into(),
                    root_path: "/tmp/a5-ws".into(),
                    rows: Ok(vec![crate::runtime::ArchivedSessionRow {
                        session_id: "s-2".into(),
                        name: "Archived".into(),
                        last_active_at: 1_790_000_000_000,
                    }]),
                }]),
                window,
                cx,
            );
        });
    });

    // The whole page, not the bare field: the page's item layout is what
    // decides how much width the rows get.
    let draw_page = |window: &mut gpui_kit::VisualTestContext, shell: &Entity<ShellWindow>| {
        let view = shell.downgrade();
        let text = text.clone();
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                // The component library asks the window for the current view
                // id during layout, so the page renders inside a view. The
                // wrapper stands in for the shell's settings column: the
                // real page gets ~640px there, and that width is what
                // decides whether the rows' actions fit.
                div()
                    .w(px(640.))
                    .h_full()
                    .child(
                        cx.new(|_| SettingsHarness {
                            text: text.clone(),
                            view: view.clone(),
                            page: settings::SettingsEntry::Archived,
                        })
                        .into_any_element(),
                    )
                    .into_any_element()
            },
        );
        window.run_until_parked();
    };
    draw_page(window, &shell);
    // A fresh load starts folded; the rows render once the group opens.
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.toggle_archived_group("/tmp/a5-ws", cx)
        });
    });
    draw_page(window, &shell);
    // The row's actions must stay inside the page: a name block that keeps
    // its natural width pushes them (and the headers' count chips) past the
    // column edge, where nothing can be clicked.
    let restore = window
        .debug_bounds("archived-restore")
        .expect("the archived row renders its restore action");
    let delete = window
        .debug_bounds("archived-delete")
        .expect("the archived row renders its delete action");
    // The page wrapper is the column's width; an action past it is
    // unreachable (the settings column clips, it does not scroll sideways).
    let page_right = px(640.);
    assert!(
        restore.origin.x + restore.size.width <= page_right,
        "restore action stays inside the page: {restore:?}"
    );
    assert!(
        delete.origin.x + delete.size.width <= page_right,
        "delete action stays inside the page: {delete:?}"
    );
}

/// G-SE13: the memory page's two menu actions settle into the shared
/// snapshot, the page's receipt line and its in-flight guard; the
/// advanced block is disclosed in place and vanishes again on collapse.
/// Renders the whole page so the sections and the disclosure prove their
/// presence, not just the state.
#[gpui_kit::test]
fn memory_page_tracks_reset_and_clear_settlements(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    let draw_page = |window: &mut gpui_kit::VisualTestContext, shell: &Entity<ShellWindow>| {
        let view = shell.downgrade();
        let text = text.clone();
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                div()
                    .w(px(640.))
                    .h_full()
                    .child(
                        cx.new(|_| SettingsHarness {
                            text: text.clone(),
                            view: view.clone(),
                            page: settings::SettingsEntry::Memory,
                        })
                        .into_any_element(),
                    )
                    .into_any_element()
            },
        );
        window.run_until_parked();
    };

    // The collapsed page renders its sections and no advanced block.
    draw_page(window, &shell);
    assert!(
        window.debug_bounds("settings-card-memory-basic").is_some(),
        "the page renders its basic section"
    );
    assert!(
        window.debug_bounds("settings-card-memory-models").is_some(),
        "the page renders its model section"
    );
    assert!(
        window
            .debug_bounds("settings-row-memory-advanced-idle-hours")
            .is_none(),
        "the advanced block starts collapsed"
    );

    // Disclosing the block is purely presentational; a reset then settles
    // the in-flight guard, replaces the snapshot with the re-read section
    // and leaves the success receipt up.
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.toggle_memory_advanced(cx);
            assert!(settings::MemoryPageState::current(cx).advanced_open);

            settings::MemoryPageState::mutate(cx, |state| state.busy = true);
            let mut restored = openbitfun_core::service::config::types::MemoriesConfig::default();
            restored.generate_memories = false;
            restored.max_unused_days = 120;
            shell.apply_runtime_event(
                RuntimeEvent::MemorySettingsReset(Ok(Box::new(restored.clone()))),
                window,
                cx,
            );
            assert!(
                !settings::MemoryPageState::current(cx).busy,
                "the guard clears with the receipt"
            );
            let snapshot = settings::ShellConfig::current(cx).memories;
            assert!(!snapshot.generate_memories);
            assert_eq!(snapshot.max_unused_days, 120);
            assert!(
                settings::MemoryNotice::current(cx).is_some_and(|(ok, _)| ok),
                "the reset states its success"
            );
        });
    });
    draw_page(window, &shell);
    assert!(
        window
            .debug_bounds("settings-row-memory-advanced-idle-hours")
            .is_some(),
        "the disclosed block is on the page"
    );

    // A failed clear reports itself, and collapsing hides the block again.
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            settings::MemoryPageState::mutate(cx, |state| state.busy = true);
            shell.apply_runtime_event(
                RuntimeEvent::MemoryCleared(Err("disk busy".into())),
                window,
                cx,
            );
            assert!(!settings::MemoryPageState::current(cx).busy);
            assert!(
                settings::MemoryNotice::current(cx).is_some_and(|(ok, _)| !ok),
                "a failure surfaces instead of a silent no-op"
            );
            shell.toggle_memory_advanced(cx);
            assert!(!settings::MemoryPageState::current(cx).advanced_open);
        });
    });
    draw_page(window, &shell);
    assert!(
        window
            .debug_bounds("settings-row-memory-advanced-idle-hours")
            .is_none(),
        "collapsing removes the block"
    );
}

/// E3-B4: the memory page carries the web page's session sections above
/// the memory cards. The fixed-mode row follows the strategy and the
/// session-title model row follows its enable switch, exactly as the web
/// page renders them; the search page keeps only its jump card.
#[gpui_kit::test]
fn memory_page_renders_the_session_sections_in_web_order(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    let draw_page = |window: &mut gpui_kit::VisualTestContext,
                     shell: &Entity<ShellWindow>,
                     page: settings::SettingsEntry| {
        let view = shell.downgrade();
        let text = text.clone();
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                div()
                    .w(px(640.))
                    .h_full()
                    .child(
                        cx.new(|_| SettingsHarness {
                            text: text.clone(),
                            view: view.clone(),
                            page,
                        })
                        .into_any_element(),
                    )
                    .into_any_element()
            },
        );
        window.run_until_parked();
    };

    // The startup default: follow-last strategy, titles enabled on the
    // fast model — so the fixed-mode row is absent and the model row is up.
    draw_page(window, &shell, settings::SettingsEntry::Memory);
    assert!(
        window
            .debug_bounds("settings-card-memory-harness")
            .is_some(),
        "the page opens with the default execution mode card"
    );
    assert!(
        window
            .debug_bounds("settings-card-memory-session-title")
            .is_some(),
        "the session title card follows it"
    );
    assert!(
        window
            .debug_bounds("settings-row-memory-harness-fixed")
            .is_none(),
        "follow-last keeps the fixed-mode row hidden"
    );
    assert!(
        window
            .debug_bounds("settings-row-memory-session-title-model")
            .is_some(),
        "enabled titles show their model row"
    );

    // Disabling titles removes the model row; a fixed strategy adds the
    // mode row — both conditional rows follow the web page.
    window.update(|_, cx| {
        cx.global_mut::<settings::ShellConfig>()
            .session_title_enabled = false;
    });
    draw_page(window, &shell, settings::SettingsEntry::Memory);
    assert!(
        window
            .debug_bounds("settings-row-memory-session-title-model")
            .is_none(),
        "a disabled title section hides its model row"
    );

    window.update(|_, cx| {
        let config = cx.global_mut::<settings::ShellConfig>();
        config.session_title_enabled = true;
        config.flow_chat = serde_json::json!({
            "default_mode_strategy": "fixed",
            "default_mode_id": "Standard",
        });
    });
    draw_page(window, &shell, settings::SettingsEntry::Memory);
    assert!(
        window
            .debug_bounds("settings-row-memory-harness-fixed")
            .is_some(),
        "a fixed strategy adds its mode row"
    );

    // The moved sections are gone from the search page.
    draw_page(window, &shell, settings::SettingsEntry::SessionSearch);
    assert!(
        window
            .debug_bounds("settings-card-session-search-open")
            .is_some(),
        "the search page keeps its jump card"
    );
    assert!(
        window
            .debug_bounds("settings-card-session-search-harness")
            .is_none(),
        "the harness card left the search page"
    );
}

/// E3-B5: the ACP page opens on the local registry view — presets plus
/// configured clients, each with its status — and the advanced-JSON card
/// only renders once the view switches (the web page's TabGroup over
/// `registry` and `json`).
#[gpui_kit::test]
fn acp_page_renders_the_local_and_json_views(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    cx.update(|cx| {
        // Codex is configured and probed (enabled → the mode picker);
        // `myagent` is a custom configured client; the presets without a
        // config render as ready rows with the add action.
        cx.set_global(crate::acp::AcpCatalog {
            loaded: true,
            clients: vec![
                acp_test_client("codex", "Codex"),
                acp_test_client("myagent", "My Agent"),
            ],
            probes: vec![acp_test_probe("codex", true)],
            probing: false,
        });
        cx.set_global(crate::acp::AcpPageState::default());
    });
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();
    let view = shell.downgrade();

    let draw_page = |window: &mut gpui_kit::VisualTestContext| {
        let text = text.clone();
        let view = view.clone();
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                div()
                    .w(px(640.))
                    .h_full()
                    .child(
                        cx.new(|_| SettingsHarness {
                            text: text.clone(),
                            view: view.clone(),
                            page: settings::SettingsEntry::ExternalAgents,
                        })
                        .into_any_element(),
                    )
                    .into_any_element()
            },
        );
        window.run_until_parked();
    };

    draw_page(window);
    assert!(
        window.debug_bounds("settings-card-acp-registry").is_some(),
        "the page opens on the local registry view"
    );
    assert!(
        window
            .debug_bounds("settings-row-acp-agent-codex")
            .is_some(),
        "the configured agent renders its registry row"
    );
    assert!(
        window
            .debug_bounds("settings-row-acp-agent-opencode")
            .is_some(),
        "unconfigured presets keep their rows"
    );
    assert!(
        window
            .debug_bounds("settings-row-acp-agent-myagent")
            .is_some(),
        "custom configured clients render after the presets"
    );
    assert!(
        window.debug_bounds("settings-card-acp-json").is_none(),
        "the advanced-JSON card stays behind its view"
    );

    window.update(|_, cx| {
        crate::acp::AcpPageState::mutate(cx, |state| state.view = crate::acp::AcpView::Json);
    });
    draw_page(window);
    assert!(
        window.debug_bounds("settings-card-acp-json").is_some(),
        "the advanced-JSON view renders its card"
    );
    assert!(
        window
            .debug_bounds("settings-row-acp-json-editor")
            .is_some(),
        "the JSON editor row renders in the advanced view"
    );
    assert!(
        window.debug_bounds("settings-card-acp-registry").is_none(),
        "the registry card stays behind the local view"
    );
}

/// E3-B5 gap: the legacy tool-confirmation warning rides above both views
/// while a client still stores `reject_once`, and clears once the stored
/// value is ask (web `pendingPermissionMigration`); the docs action lives
/// in the view-controls row on both views.
#[gpui_kit::test]
fn acp_page_warns_about_legacy_reject_modes(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    cx.update(|cx| {
        let mut client = acp_test_client("legacy", "Legacy Agent");
        client.permission_mode = openbitfun_acp::client::AcpClientPermissionMode::RejectOnce;
        cx.set_global(crate::acp::AcpCatalog {
            loaded: true,
            clients: vec![client],
            probes: Vec::new(),
            probing: false,
        });
        cx.set_global(crate::acp::AcpPageState::default());
    });
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();
    let view = shell.downgrade();

    let draw_page = |window: &mut gpui_kit::VisualTestContext| {
        let text = text.clone();
        let view = view.clone();
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                div()
                    .w(px(640.))
                    .h_full()
                    .child(
                        cx.new(|_| SettingsHarness {
                            text: text.clone(),
                            view: view.clone(),
                            page: settings::SettingsEntry::ExternalAgents,
                        })
                        .into_any_element(),
                    )
                    .into_any_element()
            },
        );
        window.run_until_parked();
    };

    draw_page(window);
    assert!(
        window.debug_bounds("acp-legacy-warning").is_some(),
        "a stored reject_once shows the migration warning"
    );
    assert!(
        window.debug_bounds("acp-docs-button").is_some(),
        "the docs action renders in the view-controls row"
    );

    window.update(|_, cx| {
        crate::acp::AcpPageState::mutate(cx, |state| state.view = crate::acp::AcpView::Json);
    });
    draw_page(window);
    assert!(
        window.debug_bounds("acp-legacy-warning").is_some(),
        "the warning rides above the advanced-JSON view too"
    );

    window.update(|_, cx| {
        for client in &mut cx.global_mut::<crate::acp::AcpCatalog>().clients {
            client.permission_mode = openbitfun_acp::client::AcpClientPermissionMode::Ask;
        }
    });
    draw_page(window);
    assert!(
        window.debug_bounds("acp-legacy-warning").is_none(),
        "the warning clears once no client stores reject_once"
    );
}

/// G-SE7: the shortcuts page documents exactly what is bound. The two
/// global rows come from the same table `install_global_shortcuts` binds,
/// so the test binds the table and asks the window's keymap back what each
/// action resolves to — a row whose stroke had drifted from the binding
/// would fail here.
#[gpui_kit::test]
fn shortcuts_page_matches_the_bound_globals(cx: &mut gpui_kit::TestAppContext) {
    // `as_keystroke` lives on a helper trait outside the gpui prelude.
    use gpui_kit::AsKeystroke as _;

    install_shell_globals(cx);
    cx.update(|cx| settings::install_global_shortcuts(cx));

    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
    let text = sample_shell_text();

    for shortcut in settings::GLOBAL_SHORTCUTS {
        let keystroke = gpui_kit::Keystroke::parse(shortcut.stroke)
            .unwrap_or_else(|error| panic!("{} does not parse: {error}", shortcut.stroke));
        let action = (shortcut.action)();
        let bound =
            window.update(|window, _cx| window.highest_precedence_binding_for_action(&*action));
        let bound = bound
            .unwrap_or_else(|| panic!("{} is not bound in the window keymap", shortcut.stroke));
        let first = bound
            .keystrokes()
            .first()
            .expect("a bound keybinding carries a keystroke")
            .as_keystroke()
            .clone();
        assert_eq!(
            gpui_kit::component::kbd::Kbd::format(&first),
            gpui_kit::component::kbd::Kbd::format(&keystroke),
            "the documented key and the bound key agree for {}",
            shortcut.stroke
        );
        // The label the row shows comes from the shell copy, never empty.
        assert!(!(shortcut.label)(&text).is_empty());
    }

    // The page renders the table: the global rows and the terminal pair
    // prove both a keycap row and a note row made it into the layout.
    let view = shell.downgrade();
    window.draw(
        point(px(0.), px(0.)),
        size(px(900.), px(600.)),
        move |_, cx| {
            div()
                .w(px(640.))
                .h_full()
                .child(
                    cx.new(|_| SettingsHarness {
                        text: text.clone(),
                        view: view.clone(),
                        page: settings::SettingsEntry::Shortcuts,
                    })
                    .into_any_element(),
                )
                .into_any_element()
        },
    );
    window.run_until_parked();
    assert!(
        window
            .debug_bounds("settings-row-shortcut-ctrl-k")
            .is_some(),
        "the global table renders its search row"
    );
    assert!(
        window
            .debug_bounds("settings-row-shortcut-ctrl-shift-c")
            .is_some(),
        "the terminal section renders its copy row"
    );
}

/// G-SE30: the archived page's projection — groups ordered by their newest
/// member (the web sorts every entry by `lastActiveAt` before grouping),
/// workspaces without archived rows dropped, `name · path` labels (just the
/// path when the name is it), one failure line per unreadable workspace,
/// and previous rows kept for a workspace whose background re-read failed
/// (web `loadArchived`).
#[gpui_kit::test]
fn archived_projection_groups_order_and_survive_a_failed_refresh(
    cx: &mut gpui_kit::TestAppContext,
) {
    install_shell_globals(cx);
    let archived_row = |id: &str, at: u64| crate::runtime::ArchivedSessionRow {
        session_id: id.to_string(),
        name: id.to_string(),
        last_active_at: at,
    };
    cx.update(|cx| {
        let groups = vec![
            crate::runtime::ArchivedWorkspaceGroup {
                workspace_id: "ws-old".into(),
                name: "old".into(),
                root_path: "/home/zero/old".into(),
                rows: Ok(vec![archived_row("s-old", 100)]),
            },
            crate::runtime::ArchivedWorkspaceGroup {
                workspace_id: "ws-new".into(),
                name: "new".into(),
                root_path: "/home/zero/new".into(),
                rows: Ok(vec![
                    archived_row("s-new-1", 900),
                    archived_row("s-new-2", 300),
                ]),
            },
            crate::runtime::ArchivedWorkspaceGroup {
                workspace_id: "ws-empty".into(),
                name: "empty".into(),
                root_path: "/home/zero/empty".into(),
                rows: Ok(Vec::new()),
            },
            crate::runtime::ArchivedWorkspaceGroup {
                workspace_id: "ws-nameless".into(),
                name: "/home/zero/nameless".into(),
                root_path: "/home/zero/nameless".into(),
                rows: Ok(vec![archived_row("s-nameless", 500)]),
            },
            crate::runtime::ArchivedWorkspaceGroup {
                workspace_id: "ws-broken".into(),
                name: "broken".into(),
                root_path: "/home/zero/broken".into(),
                rows: Err("store unreadable".into()),
            },
        ];
        let (composed, failures) = compose_archived_groups(groups, &[], true, cx);
        assert_eq!(failures.len(), 1, "one line per unreadable workspace");
        let ids: Vec<&str> = composed
            .iter()
            .map(|group| group.workspace_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["ws-new", "ws-nameless", "ws-old"],
            "newest member leads; workspaces without archived rows drop out"
        );
        assert_eq!(composed[0].label, "new · /home/zero/new");
        assert_eq!(
            composed[1].label, "/home/zero/nameless",
            "a name that already is the path labels as the path"
        );
        let rows: Vec<&str> = composed[0]
            .rows
            .iter()
            .map(|row| row.session_id.as_str())
            .collect();
        assert_eq!(rows, vec!["s-new-1", "s-new-2"], "rows are newest first");

        // A background refresh keeps the failed workspace's previous rows
        // and still reports the failure.
        let previous = composed.clone();
        let (composed, failures) = compose_archived_groups(
            vec![crate::runtime::ArchivedWorkspaceGroup {
                workspace_id: "ws-new".into(),
                name: "new".into(),
                root_path: "/home/zero/new".into(),
                rows: Err("store unreadable".into()),
            }],
            &previous,
            false,
            cx,
        );
        assert_eq!(failures.len(), 1);
        assert_eq!(composed.len(), 1);
        assert_eq!(composed[0].rows.len(), 2, "previous rows stay visible");
    });
}

/// Background subagent activity (T2-6): a linked child whose task tool
/// started with `run_in_background` badges the parent row while its turn
/// runs, and the badge clears when the child settles.
#[gpui_kit::test]
fn background_subagent_badges_track_the_child_lifecycle(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, _rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, _rx, window, cx)));
    let task_card = |background: bool| RuntimeEvent::ToolUpdate {
        session_id: "parent".into(),
        card: crate::runtime::ToolCard {
            key: "tool-call-1".into(),
            call_id: "call-1".into(),
            tool_name: "task".into(),
            status: crate::runtime::ToolStatus::Running,
            facts: transcript::ToolFacts::empty(),
            result_success: None,
            subagent_session: None,
            background,
        },
    };
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(task_card(true), window, cx);
            shell.apply_runtime_event(
                RuntimeEvent::SubagentLinked {
                    parent_session_id: "parent".into(),
                    parent_tool_call_id: "call-1".into(),
                    subagent_session_id: "child-1".into(),
                    agent_type: None,
                },
                window,
                cx,
            );
            // The child has not started: nothing is running yet.
            assert_eq!(shell.session_activity_badges("parent"), (0, None));
            shell.apply_runtime_event(
                RuntimeEvent::TurnStarted {
                    session_id: "child-1".into(),
                    turn_id: "t1".into(),
                },
                window,
                cx,
            );
            assert_eq!(shell.session_activity_badges("parent"), (1, None));
            // The child outlives the parent's turn: still activity.
            shell.apply_runtime_event(
                RuntimeEvent::TurnFinished {
                    session_id: "parent".into(),
                    error: None,
                    duration_ms: None,
                    cancelled: false,
                },
                window,
                cx,
            );
            assert_eq!(shell.session_activity_badges("parent"), (1, None));
            shell.apply_runtime_event(
                RuntimeEvent::TurnFinished {
                    session_id: "child-1".into(),
                    error: None,
                    duration_ms: None,
                    cancelled: false,
                },
                window,
                cx,
            );
            assert_eq!(shell.session_activity_badges("parent"), (0, None));
            // A non-background task tool never badges while the parent
            // turn is live (the web store's run_in_background signal).
            shell.apply_runtime_event(task_card(false), window, cx);
            shell.apply_runtime_event(
                RuntimeEvent::SubagentLinked {
                    parent_session_id: "parent".into(),
                    parent_tool_call_id: "call-2".into(),
                    subagent_session_id: "child-2".into(),
                    agent_type: None,
                },
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::TurnStarted {
                    session_id: "parent".into(),
                    turn_id: "t2".into(),
                },
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::TurnStarted {
                    session_id: "child-2".into(),
                    turn_id: "t3".into(),
                },
                window,
                cx,
            );
            assert_eq!(shell.session_activity_badges("parent"), (0, None));
        });
    });
}

/// A review-family agent type surfaces the review badge on the parent
/// row once its run goes background (parent turn settled).
#[gpui_kit::test]
fn review_subagents_classify_into_the_review_badge(cx: &mut gpui_kit::TestAppContext) {
    assert_eq!(
        review_activity_kind("ReviewWorker"),
        Some(ReviewActivityKind::Review)
    );
    assert_eq!(
        review_activity_kind("ReviewSecurity"),
        Some(ReviewActivityKind::Review)
    );
    assert_eq!(
        review_activity_kind("CodeReview"),
        Some(ReviewActivityKind::Review)
    );
    assert_eq!(
        review_activity_kind("DeepReview"),
        Some(ReviewActivityKind::DeepReview)
    );
    assert_eq!(review_activity_kind("Standard"), None);
    assert_eq!(review_activity_kind("CodebaseResearcher"), None);

    install_shell_globals(cx);
    let (tx, _rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, _rx, window, cx)));
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SubagentLinked {
                    parent_session_id: "parent".into(),
                    parent_tool_call_id: "call-1".into(),
                    subagent_session_id: "reviewer".into(),
                    agent_type: Some("ReviewWorker".into()),
                },
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::TurnStarted {
                    session_id: "reviewer".into(),
                    turn_id: "t1".into(),
                },
                window,
                cx,
            );
            // Foreground semantics: the parent turn is still live and the
            // task tool never ran in background, so no badge — matching
            // the web store.
            shell.apply_runtime_event(
                RuntimeEvent::TurnStarted {
                    session_id: "parent".into(),
                    turn_id: "t0".into(),
                },
                window,
                cx,
            );
            assert_eq!(shell.session_activity_badges("parent"), (0, None));
            // Once the parent turn settles, the still-running review run
            // has outlived it and shows the review badge.
            shell.apply_runtime_event(
                RuntimeEvent::TurnFinished {
                    session_id: "parent".into(),
                    error: None,
                    duration_ms: None,
                    cancelled: false,
                },
                window,
                cx,
            );
            assert_eq!(
                shell.session_activity_badges("parent"),
                (1, Some(ReviewActivityKind::Review))
            );
        });
    });
}

/// Number of session rows the sidebar would render with the current
/// archive toggle.
fn visible_session_rows(
    window: &mut gpui_kit::VisualTestContext,
    shell: &Entity<ShellWindow>,
    text: &ShellText,
) -> usize {
    let text = text.clone();
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell
                .render_session_entries(&text, cx)
                .into_iter()
                .flat_map(|section| match section {
                    SidebarSection::Rows(entries) => entries,
                    SidebarSection::Item(entry) => vec![entry],
                })
                .filter(|entry| matches!(entry, SidebarEntry::Session { depth: 0, .. }))
                .count()
        })
    })
}

fn install_shell_globals(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let presets = theme::load_presets();
        let active = presets
            .iter()
            .position(|preset| preset.id == DEFAULT_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active) {
            preset.apply(cx);
        }
        cx.set_global(ShellState {
            presets,
            active_preset: active,
        });
        cx.set_global(settings::ShellConfig::startup_default(LocaleId::ZhCN));
        cx.set_global(sample_shell_text());
    });
}

/// A list-projections fixture summary: both ordering keys carry `at_ms`,
/// so a test can script the `updated` and `created` orderings.
fn sample_summary_at(
    id: &str,
    name: &str,
    at_ms: u64,
) -> openbitfun_runtime_ports::AgentSessionSummary {
    let mut summary = sample_summary(id, name);
    summary.created_at_ms = at_ms;
    summary.last_active_at_ms = at_ms;
    summary
}

fn sample_summary(id: &str, name: &str) -> openbitfun_runtime_ports::AgentSessionSummary {
    openbitfun_runtime_ports::AgentSessionSummary {
        session_id: id.into(),
        session_name: name.into(),
        agent_type: "Standard".into(),
        model_id: None,
        reasoning_preset: None,
        last_user_dialog_agent_type: None,
        last_submitted_agent_type: None,
        turn_count: 0,
        created_at_ms: 0,
        last_active_at_ms: 0,
    }
}

/// One `ask_user` mailbox payload shaped like the tool's input plus the
/// timing metadata the runtime adds.
fn question_payload(questions: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "questions": questions,
        "responseHostNowMs": 1_700_000_000_000u64,
        "responseDeadlineMs": serde_json::Value::Null,
    })
}

fn sample_question(tool_id: &str, payload: serde_json::Value) -> PendingUserQuestion {
    PendingUserQuestion::new(
        tool_id,
        "session-1",
        Some("turn-1".to_string()),
        None,
        payload,
    )
}

/// Deterministic stand-in for the localized "n of m" label.
fn test_question_label(index: usize, total: usize) -> String {
    format!("Question {index} / {total}")
}

fn two_choice_payload(multi_select: bool) -> serde_json::Value {
    question_payload(serde_json::json!([{
        "question": "Which path should be used?",
        "header": "Path",
        "options": [
            { "label": "A", "description": "Use A" },
            { "label": "B", "description": "Use B" }
        ],
        "multiSelect": multi_select
    }]))
}

#[test]
fn question_payload_parses_into_render_items() {
    let item = compose_question_item(
        sample_question("tool-1", two_choice_payload(false)),
        test_question_label,
    );
    assert_eq!(item.tool_id, "tool-1");
    assert_eq!(item.questions.len(), 1);
    assert_eq!(item.questions[0].options.len(), 2);
    assert_eq!(item.questions[0].options[1].label, "B");
    assert!(!item.questions[0].multi_select);
    assert_eq!(item.questions[0].progress_line, "Question 1 / 1");
    assert!(!item.is_answered());
    // Timing metadata alongside the tool input is ignored.
    assert_eq!(item.selected.len(), 1);
}

#[test]
fn question_payload_without_options_is_unanswerable() {
    for questions in [
        serde_json::json!([]),
        serde_json::json!([{ "question": "q", "header": "h", "options": [] }]),
        serde_json::json!([{ "question": "q", "header": "h" }]),
    ] {
        let item = compose_question_item(
            sample_question("tool-1", question_payload(questions)),
            test_question_label,
        );
        assert!(item.questions.is_empty(), "batch must stay cancel-only");
        assert!(!item.is_answered());
        assert!(item.answers().as_object().is_some_and(|map| map.is_empty()));
    }
}

#[test]
fn question_answers_follow_the_tool_wire_shape() {
    let mut item = compose_question_item(
        sample_question("tool-1", two_choice_payload(false)),
        test_question_label,
    );
    toggle_option_selection(&mut item.selected[0], 1, false);
    assert!(item.is_answered());
    assert_eq!(item.answers(), serde_json::json!({ "0": "B" }));

    // Multi-select answers are arrays of labels, and unanswered questions
    // are omitted.
    let multi = question_payload(serde_json::json!([
        {
            "question": "Which ones?",
            "header": "Pick",
            "options": [
                { "label": "X", "description": "x" },
                { "label": "Y", "description": "y" }
            ],
            "multiSelect": true
        },
        {
            "question": "And one?",
            "header": "One",
            "options": [
                { "label": "P", "description": "p" },
                { "label": "Q", "description": "q" }
            ]
        }
    ]));
    let mut item = compose_question_item(sample_question("tool-1", multi), test_question_label);
    toggle_option_selection(&mut item.selected[0], 0, true);
    toggle_option_selection(&mut item.selected[0], 1, true);
    assert!(!item.is_answered(), "the second question has no selection");
    assert_eq!(item.answers(), serde_json::json!({ "0": ["X", "Y"] }));
}

#[test]
fn question_option_selection_toggles_or_replaces() {
    let mut selected = BTreeSet::new();
    toggle_option_selection(&mut selected, 0, false);
    toggle_option_selection(&mut selected, 1, false);
    assert_eq!(selected.iter().copied().collect::<Vec<_>>(), vec![1]);

    let mut selected = BTreeSet::new();
    toggle_option_selection(&mut selected, 0, true);
    toggle_option_selection(&mut selected, 1, true);
    assert_eq!(selected.iter().copied().collect::<Vec<_>>(), vec![0, 1]);
    toggle_option_selection(&mut selected, 0, true);
    assert_eq!(selected.iter().copied().collect::<Vec<_>>(), vec![1]);
}

/// The question queue is rebuilt from the mailbox snapshot on every
/// change: in-progress selections survive an unrelated question arriving
/// or leaving, resolved questions drop out, and the dialog slot follows
/// the queue.
#[test]
fn question_queue_rebuild_keeps_selections_and_drops_resolved() {
    let mut head = compose_question_item(
        sample_question("tool-1", two_choice_payload(false)),
        test_question_label,
    );
    toggle_option_selection(&mut head.selected[0], 1, false);
    let existing = vec![head];

    let rebuilt = rebuild_question_queue(
        &existing,
        vec![
            sample_question("tool-1", two_choice_payload(false)),
            sample_question("tool-2", two_choice_payload(true)),
        ],
        test_question_label,
    );
    assert_eq!(rebuilt.len(), 2);
    assert!(rebuilt[0].is_answered());
    assert_eq!(
        rebuilt[0].selected[0].iter().copied().collect::<Vec<_>>(),
        vec![1]
    );
    assert!(!rebuilt[1].is_answered());

    let rebuilt = rebuild_question_queue(
        &rebuilt,
        vec![sample_question("tool-2", two_choice_payload(true))],
        test_question_label,
    );
    assert_eq!(rebuilt.len(), 1);
    assert_eq!(rebuilt[0].tool_id, "tool-2");

    assert!(rebuild_question_queue(&rebuilt, Vec::new(), test_question_label).is_empty());
}

/// A workspace switch resets every workspace-scoped surface: the open
/// transcript, the session list (and its archive toggle), the terminal,
/// the changes pane, and the error lines — while a failure keeps the
/// previous workspace active and surfaces the reason.
#[gpui_kit::test]
fn workspace_switch_resets_scoped_surfaces(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::Sessions(vec![crate::runtime::SessionListRow {
                    summary: sample_summary("s-1", "First"),
                    archived: false,
                }]),
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: Vec::new(),
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
            shell.show_archived = true;
            shell.list_error = Some("stale".into());
            shell.toggle_files_view(window, cx);
        });
        inject_terminal_tab(&shell, "old-ws", window, cx);
    });
    window.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(!shell.chats.is_empty());
        assert_eq!(shell.sessions.len(), 1);
    });

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::WorkspaceSwitched(WorkspaceChoice {
                    id: "ws-2".into(),
                    name: "Second".into(),
                    root_path: "/tmp/ws-2".into(),
                }),
                window,
                cx,
            );
        });
    });
    window.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(
            shell.chats.is_empty() && shell.tabs.active_kind().is_none(),
            "transcript tabs belong to the old workspace"
        );
        assert!(
            shell.sessions.is_empty(),
            "session list reloads for the new root"
        );
        assert!(!shell.show_archived, "archive filter resets with the list");
        assert!(shell.list_error.is_none());
        assert!(
            shell.terminals.is_empty(),
            "terminal PTYs shut down with the old workspace"
        );
        assert!(!shell.terminal_open);
        assert!(shell.git.is_none());
        assert!(shell.files.is_none(), "files page binds the old workspace");
        assert_eq!(shell.tabs.active_kind(), None);
        assert!(shell.workspace_error.is_none());
    });

    // A rejected switch keeps the current workspace and shows the reason.
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::WorkspaceSwitchFailed("owned by a shared runtime".into()),
                window,
                cx,
            );
        });
    });
    window.update(|_, cx| {
        assert_eq!(
            shell.read(cx).workspace_error.as_deref(),
            Some("owned by a shared runtime")
        );
    });
}

/// The files page toggles from the footer and its entity (open tabs
/// included) survives switching back to chat; without a runtime the page
/// degrades loudly instead of rendering a dead tree.
#[gpui_kit::test]
fn files_view_toggles_and_survives_view_switches(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.toggle_files_view(window, cx);
            assert_eq!(
                shell.tabs.active_kind(),
                Some(&TabKind::Scene(tabs::SceneId::Files))
            );
            assert!(shell.files.is_some());
        });
    });
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let page = shell.files.as_ref().unwrap().read(cx);
            assert!(page.error().is_some(), "no runtime here: degrade loudly");
        });
        shell.update(cx, |shell, cx| {
            // Tab semantics (R1a): the nav row activates idempotently;
            // the tab close drops the scene tab, the page entity stays.
            shell.toggle_files_view(window, cx);
            assert_eq!(
                shell.tabs.active_kind(),
                Some(&TabKind::Scene(tabs::SceneId::Files))
            );
            assert!(shell.files.is_some(), "tabs survive the scene close");

            shell.close_top_tab(
                shell
                    .tabs
                    .index_of(&TabKind::Scene(tabs::SceneId::Files))
                    .unwrap(),
                cx,
            );
            assert_eq!(shell.tabs.active_kind(), None);

            shell.toggle_files_view(window, cx);
            assert_eq!(
                shell.tabs.active_kind(),
                Some(&TabKind::Scene(tabs::SceneId::Files))
            );
        });
    });
}

/// R2c opening policy state machine: with a session tab active the
/// resource docks into the session's right panel instead of opening a
/// scene tab, panel and main tab stay mutually exclusive, and pop-out
/// reverses the move without dropping the page entity.
#[gpui_kit::test]
fn resources_dock_into_the_session_panel_and_pop_out(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: Vec::new(),
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
            // Session tab active: Files docks, no scene tab appears.
            shell.toggle_files_view(window, cx);
            assert_eq!(shell.resource_panel, Some(ResourcePanelKind::Files));
            assert!(shell
                .tabs
                .index_of(&TabKind::Scene(tabs::SceneId::Files))
                .is_none());
            // The other kind swaps the docked panel in place.
            shell.toggle_git_view(window, cx);
            assert_eq!(shell.resource_panel, Some(ResourcePanelKind::Git));
            // Toggling the docked kind closed hides the panel.
            shell.toggle_git_view(window, cx);
            assert_eq!(shell.resource_panel, None);
            // Dock again and pop out: the main tab opens and activates.
            shell.toggle_git_view(window, cx);
            shell.pop_out_resource_panel(cx);
            assert_eq!(shell.resource_panel, None);
            assert_eq!(
                shell.tabs.active_kind(),
                Some(&TabKind::Scene(tabs::SceneId::Git))
            );
            // The two forms are exclusive: docking evicts the main tab
            // while keeping the page entity alive across the move.
            shell.dock_resource_panel(ResourcePanelKind::Git, cx);
            assert!(shell
                .tabs
                .index_of(&TabKind::Scene(tabs::SceneId::Git))
                .is_none());
            assert!(shell.git.is_some(), "the mount move keeps the entity");
        });
    });
}

/// The docked panel (R2c) lays out to the right of the sidebar over the
/// chat column with its head chrome, and pop-out removes it from the
/// session view.
/// Assistant markdown carries the desktop reader's rich-render hooks:
/// identical fenced blocks in one message each get their own toolbar
/// (element ids are scoped per block) and the block draws without the
/// duplicate-id a11y panic.
#[gpui_kit::test]
fn assistant_markdown_renders_code_block_toolbars(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    let body = "Two blocks:\n\n```rust\nfn a() {}\n```\n\n```rust\nfn a() {}\n```\n";
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: vec![ChatEntry::Assistant {
                        round_id: "round-1".into(),
                        text: body.into(),
                    }],
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
        });
    });
    // Both code blocks host a toolbar; the second draw settles the
    // markdown parse the first frame kicked off.
    for _ in 0..2 {
        let view = shell.clone();
        window.draw(
            point(px(0.), px(0.)),
            size(px(1400.), px(900.)),
            move |_, _| view.into_any_element(),
        );
        window.run_until_parked();
    }
    window.run_until_parked();
    assert!(
        window.debug_bounds("code-block-toolbar").is_some(),
        "fenced code blocks render their language/copy toolbar"
    );
}

/// Tool rows carry the desktop content actions: a path-bearing row opens
/// its file, a command row copies the whole command.
#[gpui_kit::test]
fn tool_rows_expose_open_and_copy_actions(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: vec![
                        // `Write` never joins a work run, and a command
                        // awaiting approval stays out of one too: both
                        // rows render their own card here.
                        ChatEntry::Tool(ToolCard {
                            key: "tool-call-1".into(),
                            call_id: "call-1".into(),
                            tool_name: "Write".into(),
                            status: ToolStatus::Done,
                            facts: transcript::facts_from_input(
                                "Write",
                                &serde_json::json!({ "file_path": "src/main.rs" }),
                            ),
                            result_success: None,
                            subagent_session: None,
                            background: false,
                        }),
                        ChatEntry::Tool(ToolCard {
                            key: "tool-call-2".into(),
                            call_id: "call-2".into(),
                            tool_name: "ExecCommand".into(),
                            status: ToolStatus::WaitingConfirm,
                            facts: transcript::facts_from_input(
                                "ExecCommand",
                                &serde_json::json!({ "cmd": "cargo test --lib" }),
                            ),
                            result_success: None,
                            subagent_session: None,
                            background: false,
                        }),
                    ],
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
        });
    });
    let view = shell.clone();
    window.draw(
        point(px(0.), px(0.)),
        size(px(1400.), px(900.)),
        move |_, _| view.into_any_element(),
    );
    window.run_until_parked();
    assert!(
        window.debug_bounds("tool-row-open").is_some(),
        "the Write row opens its file"
    );
    assert!(
        window.debug_bounds("tool-row-copy").is_some(),
        "the command row copies the whole command"
    );
}

/// M24: an exploration/execution run collapses into one summary chip, its
/// member rows render empty, and the reader's toggle flips it open.
#[gpui_kit::test]
fn work_runs_collapse_into_a_summary_chip(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    let entries = vec![
        ChatEntry::User {
            text: "port the slice".into(),
            turn_id: Some("turn-1".into()),
        },
        ChatEntry::Tool(ToolCard {
            key: "tool-call-1".into(),
            call_id: "call-1".into(),
            tool_name: "Read".into(),
            status: ToolStatus::Done,
            facts: transcript::facts_from_input(
                "Read",
                &serde_json::json!({ "file_path": "src/main.rs" }),
            ),
            result_success: None,
            subagent_session: None,
            background: false,
        }),
        ChatEntry::Tool(ToolCard {
            key: "tool-call-2".into(),
            call_id: "call-2".into(),
            tool_name: "ExecCommand".into(),
            status: ToolStatus::Done,
            facts: transcript::facts_from_input(
                "ExecCommand",
                &serde_json::json!({ "cmd": "cargo test --lib" }),
            ),
            result_success: None,
            subagent_session: None,
            background: false,
        }),
    ];
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries,
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
        });
    });

    let view = shell.clone();
    window.draw(
        point(px(0.), px(0.)),
        size(px(1400.), px(900.)),
        move |_, _| view.into_any_element(),
    );
    window.run_until_parked();
    assert!(
        window.debug_bounds("transcript-group").is_some(),
        "the settled run renders its summary chip"
    );

    // Flipping the run open is a reader choice that survives the frame:
    // the chat records it and the run's own rows become visible.
    window.update(|_, cx| {
        shell.update(cx, |shell, _| {
            let chat = shell.chat_mut_for("s-1").expect("chat open");
            let plan = transcript::TranscriptGroups::build(&chat.entries);
            let group = plan.group_of(1).expect("run covers both calls").clone();
            assert_eq!(group.len, 2);
            assert!(!chat.group_expanded(&group), "settled runs fold by default");
            chat.toggle_group(&group);
            assert!(chat.group_expanded(&group));
            assert_eq!(chat.expanded_groups.len(), 1);
        });
    });
}

/// M24: a restored transcript has no live link events, but its lineage
/// entries carry `parent_tool_call_id` — the shell seeds the subagent map
/// from them, so an agent row resolves the identity the web UI shows.
#[gpui_kit::test]
fn lineage_entries_seed_subagent_identity(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    let spawn = ToolCard {
        key: "tool-call-1".into(),
        call_id: "call-1".into(),
        tool_name: "AgentSpawn".into(),
        status: ToolStatus::Done,
        facts: transcript::facts_from_input(
            "AgentSpawn",
            &serde_json::json!({
                "agent_id": "parser-review",
                "agent_type": "SwarmWorker",
                "model_id": "glm-5.3",
                "prompt": "port the parser",
            }),
        ),
        result_success: None,
        subagent_session: None,
        background: false,
    };
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: vec![ChatEntry::Tool(spawn.clone())],
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
            // The fixture session id and its launcher entry: the same
            // shape the zenix parity session's lineage carries.
            let child_session = "0ea023ee-9e67-4300-9e89-3e9586438292";
            shell.apply_runtime_event(
                RuntimeEvent::SessionLineages(vec![
                    openbitfun_runtime_ports::AgentSessionLineageSnapshot {
                        root_session_id: "s-1".into(),
                        sessions: vec![openbitfun_runtime_ports::AgentSessionLineageEntry {
                            parent_tool_call_id: Some("call-1".into()),
                            agent_id: Some("parser-review".into()),
                            ..lineage_entry(
                                child_session,
                                "Subagent: port the parser",
                                Some("s-1"),
                                Some("SwarmWorker"),
                            )
                        }],
                    },
                ]),
                window,
                cx,
            );
        });
    });

    window.update(|_, cx| {
        shell.update(cx, |shell, _| {
            // The link map now knows the child session, seeded from the
            // lineage entry rather than a live event.
            let link = shell
                .subagent_links
                .iter()
                .find(|link| link.tool_call_id == "call-1")
                .expect("lineage link");
            assert_eq!(
                link.subagent_session_id,
                "0ea023ee-9e67-4300-9e89-3e9586438292"
            );
            let identity = shell.subagent_identity_for(&spawn).expect("identity");
            // Same seeding rule as the web resolver: the child session id
            // picks popcorn-imp (爆米花) and robot-03.
            assert_eq!(identity.seed, "0ea023ee-9e67-4300-9e89-3e9586438292");
            assert_eq!(
                subagent_identity::avatar_path(&identity.seed),
                "subagent-avatars/robot-03.svg"
            );
            let text = sample_shell_text();
            assert_eq!(
                subagent_identity::display_name(&identity.seed, &text),
                "Popcorn Imp"
            );
            assert_eq!(identity.agent_type.as_deref(), Some("SwarmWorker"));
            assert_eq!(identity.model.as_deref(), Some("glm-5.3"));

            // A wait row naming the same alias resolves through the
            // roster index without a spawn card of its own.
            let wait = ToolCard {
                key: "tool-call-2".into(),
                call_id: "call-2".into(),
                tool_name: "AgentWait".into(),
                status: ToolStatus::Done,
                facts: transcript::facts_from_input(
                    "AgentWait",
                    &serde_json::json!({ "bg_task_ids": ["parser-review_bg1"] }),
                ),
                result_success: None,
                subagent_session: None,
                background: false,
            };
            let entries = vec![ChatEntry::Tool(wait.clone())];
            let index = shell.agent_index("s-1", &entries);
            // The task id has no launch in this transcript, so the roster
            // stays empty rather than inventing an identity.
            assert!(index.roster_for(&wait).is_empty());
            // The spawn card itself indexes both ways.
            let spawn_entries = vec![ChatEntry::Tool(spawn.clone())];
            let spawn_index = shell.agent_index("s-1", &spawn_entries);
            let roster = spawn_index.roster_for(&spawn);
            assert_eq!(roster.len(), 1);
            assert_eq!(roster[0].seed, "0ea023ee-9e67-4300-9e89-3e9586438292");
        });
    });
}

#[gpui_kit::test]
fn user_bubble_wraps_and_caps_inside_the_reading_column(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    // A long CJK prompt: line-breaking must wrap it inside the bubble
    // (the R2d regression: it used to render as one clipped line).
    let long_prompt = "把如果变成真：".repeat(12);
    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: vec![
                        ChatEntry::User {
                            text: long_prompt,
                            turn_id: Some("turn-1".into()),
                        },
                        ChatEntry::Assistant {
                            round_id: "round-1".into(),
                            text: "done".into(),
                        },
                    ],
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
        });
    });
    let view = shell.clone();
    window.draw(
        point(px(0.), px(0.)),
        size(px(1400.), px(900.)),
        move |_, _| view.into_any_element(),
    );
    window.run_until_parked();
    let cap = window
        .debug_bounds("user-bubble-cap")
        .expect("user bubble cap lays out");
    // 72% of the 900px reading column; the column is centered in the
    // chat area right of the 300px sidebar.
    assert!(
        cap.size.width <= px(900. * 0.72 + 1.),
        "bubble caps at the web fraction: {:?}",
        cap
    );
    // Wrapped CJK at text_sm/1.625 line height is ~21px per line; the
    // 168-char prompt cannot fit three 648px lines (clamp) — anything
    // below four lines means it rendered as one clipped line again.
    assert!(
        cap.size.height > px(60.),
        "bubble wraps to multiple lines: {:?}",
        cap
    );
}

#[gpui_kit::test]
fn turn_footer_stays_inside_the_reading_column(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: vec![
                        ChatEntry::User {
                            text: "rewrite the docs".into(),
                            turn_id: Some("turn-1".into()),
                        },
                        ChatEntry::Assistant {
                            round_id: "round-1".into(),
                            text: "docs rewritten".into(),
                        },
                    ],
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
        });
    });
    let view = shell.clone();
    // A 1150px window: the reading column squeezes below its 900px max
    // (the Xvfb smoke geometry that first showed the overflow).
    window.draw(
        point(px(0.), px(0.)),
        size(px(1150.), px(838.)),
        move |_, _| view.into_any_element(),
    );
    window.run_until_parked();
    let footer = window
        .debug_bounds("turn-footer")
        .expect("finished turn hosts the footer");
    let row = window
        .debug_bounds("chat-row")
        .expect("reading-column row lays out");
    assert!(
        footer.origin.x >= row.origin.x
            && footer.origin.x + footer.size.width <= row.origin.x + row.size.width,
        "footer sits inside the reading column: footer {:?} row {:?}",
        footer,
        row
    );
}

#[gpui_kit::test]
fn resource_panel_lays_out_right_of_the_chat_column(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: Vec::new(),
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
            shell.toggle_files_view(window, cx);
        });
    });
    let view = shell.clone();
    window.draw(
        point(px(0.), px(0.)),
        size(px(1400.), px(900.)),
        move |_, _| view.into_any_element(),
    );
    window.run_until_parked();
    let sidebar = window
        .debug_bounds("shell-body-sidebar")
        .expect("session view lays out");
    let panel = window
        .debug_bounds("resource-panel")
        .expect("docked panel lays out with the session");
    assert!(
        panel.origin.x > sidebar.origin.x + sidebar.size.width,
        "panel sits right of the sidebar: panel {:?} sidebar {:?}",
        panel,
        sidebar
    );
    let head = window
        .debug_bounds("resource-panel-head")
        .expect("panel head lays out");
    assert!(head.size.height > px(20.), "head height: {:?}", head.size);
    assert!(
        panel.size.width >= px(MIN_RESOURCE_PANEL_WIDTH),
        "panel width: {:?}",
        panel.size
    );

    window.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.pop_out_resource_panel(cx));
    });
    window.draw(
        point(px(0.), px(0.)),
        size(px(1400.), px(900.)),
        move |_, _| shell.into_any_element(),
    );
    window.run_until_parked();
    assert!(window.debug_bounds("resource-panel").is_none());
}

/// G-F8: the panel head's swap action moves the resource panel to the
/// other side of the chat column, and the maximize action hands it the
/// whole scene body (web `swapPanes` / `toggleMaximized`).
#[gpui_kit::test]
fn resource_panel_swaps_sides_and_maximizes(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                    reload: false,
                    session_id: "s-1".into(),
                    name: "First".into(),
                    agent_type: "Standard".into(),
                    model_id: None,
                    entries: Vec::new(),
                    busy: false,
                    turn_times: Default::default(),
                }),
                window,
                cx,
            );
            // The scene top bar's panel button path: ensure the page, then
            // dock. Docking without the page renders an empty body.
            shell.ensure_resource_page(ResourcePanelKind::Files, window, cx);
            shell.dock_resource_panel(ResourcePanelKind::Files, cx);
            assert!(
                shell.files.is_some(),
                "the top-bar dock creates the files page"
            );
            assert_eq!(shell.resource_panel_side, PanelSide::Right);
        });
    });
    fn draw(window: &mut gpui_kit::VisualTestContext, shell: &Entity<ShellWindow>) {
        window.draw(point(px(0.), px(0.)), size(px(1400.), px(900.)), {
            let shell = shell.clone();
            move |_, _| shell.into_any_element()
        });
        window.run_until_parked();
    }
    draw(window, &shell);
    let chat = window
        .debug_bounds("chat-column")
        .expect("chat column lays out");
    let panel = window
        .debug_bounds("resource-panel")
        .expect("panel lays out");
    assert!(
        panel.origin.x >= chat.origin.x + chat.size.width,
        "default dock is right of the chat column: panel {:?} chat {:?}",
        panel,
        chat
    );

    window.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.swap_resource_panel_side(cx));
    });
    draw(window, &shell);
    let chat = window
        .debug_bounds("chat-column")
        .expect("chat column lays out");
    let panel = window
        .debug_bounds("resource-panel")
        .expect("panel lays out");
    window.update(|_, cx| {
        shell.update(cx, |shell, _| {
            assert_eq!(shell.resource_panel_side, PanelSide::Left);
        });
    });
    assert!(
        panel.origin.x + panel.size.width <= chat.origin.x,
        "after the swap the panel sits left of the chat column: panel {:?} chat {:?}",
        panel,
        chat
    );

    window.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.toggle_resource_panel_maximized(cx));
    });
    draw(window, &shell);
    assert!(
        window.debug_bounds("chat-column").is_none(),
        "maximized: the chat column steps aside"
    );
    let panel = window
        .debug_bounds("resource-panel")
        .expect("maximized panel lays out");
    let scene = window
        .debug_bounds("shell-scene-column")
        .expect("the scene column lays out");
    assert!(
        (panel.size.width - scene.size.width).abs() < px(2.),
        "maximized panel fills the scene body: panel {:?} scene {:?}",
        panel.size,
        scene.size
    );

    // Closing the panel drops the maximize with it (a maximized empty
    // scene body would have no way back).
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.close_resource_panel(cx));
    });
    draw(window, &shell);
    assert!(window.debug_bounds("resource-panel").is_none());
    assert!(
        window.debug_bounds("chat-column").is_some(),
        "the chat column returns when the panel closes"
    );
    window.update(|_, cx| {
        shell.update(cx, |shell, _| assert!(!shell.resource_panel_maximized));
    });
}

/// G-X6: the notification center lists the unread completions the tray
/// badge counts, and its clear-all action empties them in one step.
#[gpui_kit::test]
fn notification_button_lists_and_clears_unread_turns(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            for (session_id, failed) in [("s-1", false), ("s-2", true)] {
                shell.apply_runtime_event(
                    RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                        reload: false,
                        session_id: session_id.into(),
                        name: format!("Session {session_id}"),
                        agent_type: "Standard".into(),
                        model_id: None,
                        entries: Vec::new(),
                        busy: false,
                        turn_times: Default::default(),
                    }),
                    window,
                    cx,
                );
                // A completion that lands while another session is on
                // screen is what the badge (and the panel) counts.
                shell.record_unread_turn(session_id, failed, window, cx);
                if !shell.unread_turns.contains_key(session_id) {
                    // The headless window counts as active, so the public
                    // gate refuses; seed the map the same way the
                    // production path leaves it.
                    shell
                        .unread_turns
                        .insert(session_id.to_string(), (1, failed));
                }
            }
            assert_eq!(shell.unread_turns.len(), 2);
        });
    });
    window.draw(point(px(0.), px(0.)), size(px(1400.), px(900.)), {
        let shell = shell.clone();
        move |_, _| shell.into_any_element()
    });
    window.run_until_parked();
    assert!(
        window.debug_bounds("top-bar-notifications").is_some(),
        "the bell renders in the scene top bar"
    );
    assert!(
        window.debug_bounds("notification-badge").is_some(),
        "unread completions show the dot"
    );

    window.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.clear_all_unread(cx));
    });
    window.update(|_, cx| {
        shell.update(cx, |shell, _| assert!(shell.unread_turns.is_empty()));
    });
    window.draw(point(px(0.), px(0.)), size(px(1400.), px(900.)), {
        let shell = shell.clone();
        move |_, _| shell.into_any_element()
    });
    window.run_until_parked();
    assert!(
        window.debug_bounds("notification-badge").is_none(),
        "the dot clears with the list"
    );
}

/// T2-16 semantics: a transcript reload of an already-open pane is not an
/// open — a turn that settled while the window was inactive keeps its
/// unread count across the refresh that settlement triggers, and the pane
/// keeps its tab title and model selector (a reload carries neither); a
/// real open reads the unread away and wins over the pane facts.
#[gpui_kit::test]
fn transcript_reloads_keep_unread_turns_until_the_session_is_opened(
    cx: &mut gpui_kit::TestAppContext,
) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    let opened = |reload: bool, name: &str| crate::runtime::OpenedSession {
        session_id: "s-1".into(),
        name: name.into(),
        agent_type: "Standard".into(),
        model_id: None,
        entries: Vec::new(),
        turn_times: Default::default(),
        reload,
        busy: false,
    };

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            // The user opens the session and a turn settles while the
            // window is inactive: the completion is recorded as unread.
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(opened(false, "Session s-1")),
                window,
                cx,
            );
            assert_eq!(
                shell.chat_for("s-1").map(|chat| chat.name.as_str()),
                Some("Session s-1"),
                "an open carries the pane title"
            );
            shell.unread_turns.insert("s-1".to_string(), (1, false));
            // Settlement refreshes the open pane through the same event;
            // its empty name is not a rename.
            shell.apply_runtime_event(RuntimeEvent::SessionOpened(opened(true, "")), window, cx);
            assert!(
                shell.unread_turns.contains_key("s-1"),
                "a reload must not read the session away"
            );
            assert_eq!(
                shell.chat_for("s-1").map(|chat| chat.name.as_str()),
                Some("Session s-1"),
                "a reload must not clear the tab title"
            );
            // The user opens the session again: that is reading it, and
            // the open's facts win.
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(opened(false, "Renamed")),
                window,
                cx,
            );
            assert!(
                !shell.unread_turns.contains_key("s-1"),
                "an open clears the session's unread turns"
            );
            assert_eq!(
                shell.chat_for("s-1").map(|chat| chat.name.as_str()),
                Some("Renamed"),
                "an open's title replaces the pane's"
            );
        });
    });
}

/// A reload refreshes an open pane's transcript in place. It is not a
/// user navigation: the settling session's tab must not take the
/// foreground away from whatever the reader is looking at, and the
/// reload must not leave a hop in the back/forward history.
#[gpui_kit::test]
fn transcript_reloads_do_not_steal_the_active_tab(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    let opened = |session_id: &str, reload: bool| crate::runtime::OpenedSession {
        session_id: session_id.into(),
        name: if reload {
            String::new()
        } else {
            format!("Session {session_id}")
        },
        agent_type: "Standard".into(),
        model_id: None,
        entries: Vec::new(),
        turn_times: Default::default(),
        reload,
        busy: false,
    };

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            // The reader settles on session B, then goes back to A: the
            // forward history points at B again.
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(opened("s-a", false)),
                window,
                cx,
            );
            shell.apply_runtime_event(
                RuntimeEvent::SessionOpened(opened("s-b", false)),
                window,
                cx,
            );
            assert_eq!(shell.tabs.active_session_id(), Some("s-b"));
            assert!(shell.tabs.go_back());
            assert_eq!(shell.tabs.active_session_id(), Some("s-a"));
            assert!(shell.tabs.can_go_forward());
            // B's turn settles while A is on screen: its transcript
            // refresh lands as a reload.
            shell.apply_runtime_event(RuntimeEvent::SessionOpened(opened("s-b", true)), window, cx);
            assert_eq!(
                shell.tabs.active_session_id(),
                Some("s-a"),
                "a reload must not pull the reader onto the settling session's tab"
            );
            assert!(
                shell.tabs.can_go_forward(),
                "a reload must not record a navigation hop"
            );
            // A scene tab is not spared either: the refresh keeps it in
            // front of the reader.
            shell.tabs.open(TabKind::Scene(crate::tabs::SceneId::Tasks));
            assert_eq!(
                shell.tabs.active_kind(),
                Some(&TabKind::Scene(crate::tabs::SceneId::Tasks))
            );
            shell.apply_runtime_event(RuntimeEvent::SessionOpened(opened("s-a", true)), window, cx);
            assert_eq!(
                shell.tabs.active_kind(),
                Some(&TabKind::Scene(crate::tabs::SceneId::Tasks)),
                "a reload must keep a scene tab in front"
            );
            // The reload still refreshed the pane: a real open of it is
            // served from the state the refresh wrote.
            assert_eq!(
                shell.chat_for("s-b").map(|chat| chat.name.as_str()),
                Some("Session s-b"),
                "a reload keeps the pane it refreshed"
            );
        });
    });
}

/// A process-lifetime tokio runtime for pane construction in headless
/// tests: `TerminalPane::new` spawns its PTY pump on the given handle, so
/// the runtime must outlive the test (`pty_integration` drives the same
/// service directly).
fn test_terminal_tokio() -> tokio::runtime::Handle {
    static RT: std::sync::OnceLock<tokio::runtime::Handle> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        // A current-thread runtime on purpose: panes only fire-and-forget
        // pumps onto this handle, and a never-driven runtime queues them
        // without a worker thread. The old multi-thread runtime ran real
        // PTY tasks concurrently with parallel tests, tripping the gpui
        // test scheduler's determinism check (~40% of full-suite runs).
        // Nothing here awaits on the handle, so queued tasks staying
        // parked is exactly the behavior the tests need.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test terminal runtime");
        let handle = runtime.handle().clone();
        std::mem::forget(runtime);
        handle
    })
    .clone()
}

/// Injects one real terminal tab (spawned bash) into the shell; tab
/// lifecycle tests then run the ordinary close path end to end.
fn inject_terminal_tab(
    shell: &Entity<ShellWindow>,
    label: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let settings = settings::ShellConfig::try_current(cx)
        .unwrap_or_else(|| settings::ShellConfig::startup_default(i18n::current(cx).locale))
        .terminal;
    let pane =
        cx.new(|cx| TerminalPane::new(test_terminal_tokio(), &settings, None, None, window, cx));
    shell.update(cx, |shell, _| {
        shell.terminals.push(TerminalTab {
            pane,
            label: SharedString::from(label.to_string()),
            directory: None,
            startup_command: None,
        });
    });
}

/// The same, for a tab launched from a profile (G-T2): it carries the
/// profile's working directory and the startup command it typed.
fn inject_terminal_profile_tab(
    shell: &Entity<ShellWindow>,
    label: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let settings = settings::ShellConfig::try_current(cx)
        .unwrap_or_else(|| settings::ShellConfig::startup_default(i18n::current(cx).locale))
        .terminal;
    let pane =
        cx.new(|cx| TerminalPane::new(test_terminal_tokio(), &settings, None, None, window, cx));
    shell.update(cx, |shell, _| {
        shell.terminals.push(TerminalTab {
            pane,
            label: SharedString::from(label.to_string()),
            directory: Some("/tmp".to_string()),
            startup_command: Some("echo hi".to_string()),
        });
    });
}

/// Terminal tab lifecycle: closing the active tab moves focus to a
/// neighbor, and closing the last tab closes the whole panel (each close
/// shuts the tab's real PTY down).
#[gpui_kit::test]
fn terminal_tabs_close_adjusts_active_and_last_closes_panel(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        inject_terminal_tab(&shell, "one", window, cx);
        inject_terminal_tab(&shell, "two", window, cx);
        shell.update(cx, |shell, _cx| {
            shell.terminal_open = true;
            shell.active_terminal = 1;
            assert_eq!(shell.terminals.len(), 2);
        });
    });
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.close_terminal_tab(1, cx);
            assert_eq!(shell.terminals.len(), 1);
            assert_eq!(shell.active_terminal, 0, "focus moves to the neighbor");
            assert!(shell.terminal_open);
        });
    });
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.close_terminal_tab(0, cx);
            assert!(shell.terminals.is_empty());
            assert!(!shell.terminal_open, "the last close closes the panel");
        });
    });
    // The tabbed layout must render: the tab bar, the new-tab menu
    // button, and the active pane all lay out inside the dock.
    window.update(|window, cx| {
        inject_terminal_tab(&shell, "again", window, cx);
        shell.update(cx, |shell, _cx| shell.terminal_open = true);
    });
    window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
        let shell = shell.clone();
        move |_, _| shell.into_any_element()
    });
    window.run_until_parked();
    let dock = window
        .debug_bounds("shell-terminal-dock")
        .expect("docked terminal renders with tabs");
    assert!(dock.size.height > px(50.), "dock height: {:?}", dock.size);
}

/// G-T1: the dock divider's double click walks the web's size tiers
/// (`getNextMode`): compact → comfortable → expanded → comfortable, with
/// the panel's remembered extent following each step.
#[gpui_kit::test]
fn terminal_size_tier_cycles_between_the_web_steps(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        // The dock preference is shared with the web; the tier extents are
        // the same numbers for both docks.
        let mut config = settings::ShellConfig::try_current(cx)
            .unwrap_or_else(|| settings::ShellConfig::startup_default(i18n::current(cx).locale));
        config.terminal.panel_position = "right".to_string();
        cx.set_global(config);
        inject_terminal_tab(&shell, "one", window, cx);
        shell.update(cx, |shell, cx| {
            shell.terminal_open = true;
            assert_eq!(shell.terminal_dock(cx), TerminalDock::Right);
            // Compact sits below the threshold; the cycle takes it up one
            // tier at a time.
            shell.terminal_width = px(TERMINAL_TIER_COMPACT);
            shell.cycle_terminal_size_tier(window, cx);
            assert_eq!(shell.terminal_width, px(TERMINAL_TIER_COMFORTABLE));
            shell.cycle_terminal_size_tier(window, cx);
            assert_eq!(shell.terminal_width, px(TERMINAL_TIER_EXPANDED));
            shell.cycle_terminal_size_tier(window, cx);
            assert_eq!(
                shell.terminal_width,
                px(TERMINAL_TIER_COMFORTABLE),
                "expanded steps back down (the web never lands on collapsed)"
            );
            // A closed panel ignores the gesture.
            shell.terminal_open = false;
            shell.terminal_width = px(TERMINAL_TIER_COMPACT);
            shell.cycle_terminal_size_tier(window, cx);
            assert_eq!(shell.terminal_width, px(TERMINAL_TIER_COMPACT));
        });
    });
}

/// G-T2: a tab launched from a profile carries its startup command (the
/// row's terminal marker) and its launch directory (the row's reveal
/// action).
#[gpui_kit::test]
fn terminal_tab_row_reveals_the_launch_directory(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        let mut config = settings::ShellConfig::try_current(cx)
            .unwrap_or_else(|| settings::ShellConfig::startup_default(i18n::current(cx).locale));
        config.terminal.panel_position = "bottom".to_string();
        cx.set_global(config);
        inject_terminal_tab(&shell, "one", window, cx);
        inject_terminal_profile_tab(&shell, "dev", window, cx);
        shell.update(cx, |shell, _cx| {
            shell.terminal_open = true;
            shell.active_terminal = 1;
        });
    });
    window.draw(point(px(0.), px(0.)), size(px(1400.), px(900.)), {
        let shell = shell.clone();
        move |_, _| shell.into_any_element()
    });
    window.run_until_parked();
    assert!(
        window.debug_bounds("terminal-tab-reveal-1").is_some(),
        "the profile tab reveals the directory it was started in"
    );
}

/// Terminal profiles flow through the shared mirror: an empty id is
/// assigned on insert, the same id replaces, delete removes.
#[gpui_kit::test]
fn terminal_profiles_upsert_and_delete_flow_through_the_mirror(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    cx.update(|cx| {
        let profile = |name: &str| openbitfun_core::service::config::types::TerminalProfile {
            id: String::new(),
            name: name.to_string(),
            working_directory: "/work".to_string(),
            startup_command: "just dev".to_string(),
        };
        settings::upsert_terminal_profile(profile("dev"), cx);
        let profiles = &settings::ShellConfig::current(cx).terminal.profiles;
        assert_eq!(profiles.len(), 1);
        assert!(!profiles[0].id.is_empty(), "insert assigns an id");
        let id = profiles[0].id.clone();

        settings::upsert_terminal_profile(
            openbitfun_core::service::config::types::TerminalProfile {
                id: id.clone(),
                name: "dev2".to_string(),
                working_directory: String::new(),
                startup_command: String::new(),
            },
            cx,
        );
        let profiles = &settings::ShellConfig::current(cx).terminal.profiles;
        assert_eq!(profiles.len(), 1, "same id replaces");
        assert_eq!(profiles[0].name, "dev2");

        settings::delete_terminal_profile(&id, cx);
        assert!(settings::ShellConfig::current(cx)
            .terminal
            .profiles
            .is_empty());
    });
}

/// Search overlay state machine (R1b): open resets the query state, the
/// pump projections land for both paths, and close drops them.
#[gpui_kit::test]
fn search_overlay_opens_projects_and_closes(cx: &mut gpui_kit::TestAppContext) {
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    let window = cx.add_empty_window();
    let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

    window.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(!shell.search_overlay_open);

            shell.open_search_overlay(window, cx);
            assert!(shell.search_overlay_open);

            shell.apply_runtime_event(
                RuntimeEvent::SearchContent {
                    hits: vec![sample_workspace_hit()],
                    truncated: true,
                },
                window,
                cx,
            );
            assert_eq!(shell.search.content.len(), 1);
            assert_eq!(shell.search.content[0].path, "/w/a.rs");
            assert!(shell.search.truncated);

            shell.apply_runtime_event(
                RuntimeEvent::SearchSessions {
                    hits: vec![sample_session_hit()],
                    truncated: false,
                },
                window,
                cx,
            );
            assert_eq!(shell.search.sessions.len(), 1);
            assert_eq!(shell.search.sessions[0].session_id, "s-9");

            shell.apply_runtime_event(RuntimeEvent::SearchUnavailable(None), window, cx);
            assert!(matches!(shell.search.unavailable, Some(None)));

            // Closing drops the query state so a stale result set never
            // lingers behind the next open.
            shell.close_search_overlay(window, cx);
            assert!(!shell.search_overlay_open);
            assert!(shell.search.content.is_empty());
            assert!(shell.search.sessions.is_empty());
            assert_eq!(shell.search.query, "");
        });
    });
}

/// Search page state machine fixtures: one grouped content hit and one
/// session hit, shaped like the wire DTOs.
fn sample_workspace_hit() -> openbitfun_core::service::search::WorkspaceSearchHit {
    openbitfun_core::service::search::WorkspaceSearchHit {
        path: "/w/a.rs".to_string(),
        matches: vec![openbitfun_core::service::search::WorkspaceSearchMatch {
            location: openbitfun_core::service::search::WorkspaceSearchMatchLocation {
                line: 3,
                column: 0,
            },
            snippet: "let x = query;".to_string(),
            matched_text: "query".to_string(),
        }],
        lines: Vec::new(),
    }
}

fn sample_session_hit() -> openbitfun_runtime_ports::SessionSearchHit {
    openbitfun_runtime_ports::SessionSearchHit {
        kind: openbitfun_runtime_ports::SessionSearchHitKind::Session,
        matched_field: openbitfun_runtime_ports::SessionSearchMatchField::Title,
        session_id: "s-9".to_string(),
        session_title: "Indexed session".to_string(),
        turn_id: None,
        turn_index: None,
        snippet: "answer text".to_string(),
        archived: false,
        updated_at_ms: 0,
        score: 1.0,
    }
}

/// Approvals gate a parked tool call, so they take the single dialog slot
/// ahead of questions; the slot is empty once both queues drain.
#[test]
fn approvals_take_the_modal_slot_before_questions() {
    assert_eq!(next_modal_target(0, 0), None);
    assert_eq!(next_modal_target(1, 0), Some(ModalOwner::Permissions));
    assert_eq!(next_modal_target(0, 1), Some(ModalOwner::Questions));
    assert_eq!(next_modal_target(2, 3), Some(ModalOwner::Permissions));
}

#[test]
fn image_formats_resolve_from_safe_extensions_only() {
    assert_eq!(image_format_for_extension("png"), Some(ImageFormat::Png));
    assert_eq!(image_format_for_extension("JPEG"), Some(ImageFormat::Jpeg));
    assert_eq!(image_format_for_extension("webp"), Some(ImageFormat::Webp));
    assert_eq!(image_format_for_extension("svg"), None);
    assert_eq!(image_format_for_extension("exe"), None);
}

#[test]
fn attachment_payload_carries_a_self_contained_data_url() {
    let item = PendingAttachment {
        id: "att-1".to_string(),
        name: "shot.png".to_string(),
        mime: "image/png".to_string(),
        data_url: "data:image/png;base64,aGVsbG8=".to_string(),
        preview: Arc::new(Image::empty()),
    };
    let attachment = item.to_input_attachment();
    assert_eq!(attachment.kind, "remote_image");
    assert_eq!(attachment.id, "att-1");
    assert_eq!(
        attachment.metadata.get("dataUrl"),
        Some(&serde_json::json!("data:image/png;base64,aGVsbG8="))
    );
    // No controller-local path ever crosses the wire: peer and remote
    // runtimes decode the embedded bytes instead.
    assert!(attachment.metadata.get("imagePath").is_none());
    assert_eq!(
        attachment.metadata.get("metadata"),
        Some(&serde_json::json!({ "name": "shot.png" }))
    );
}

/// R1b sidebar nav over the R1a tab model: the row set is the desktop
/// top-scene set in NavPanel order (assistant / tasks / mini-apps /
/// extensions — the middle two degrade to honest empty-card scenes),
/// and exactly one row lights up per active tab (Settings activates
/// none — its own footer button keeps the raised variant). Files and
/// git stay workbench panels that light no row; they live in the search
/// palette. Insights has owned the fifth row since E4 batch ①. A
/// session tab lights the Chat row; no tabs light it too (the resting
/// empty state).
#[test]
fn nav_rows_map_active_state_to_tabs() {
    let actions = nav_actions();
    assert_eq!(
        actions,
        vec![
            NavAction::Chat,
            NavAction::Tasks,
            NavAction::MiniApps,
            NavAction::Extensions,
            NavAction::Insights,
        ]
    );
    let mut tabs = TabsState::default();
    for kind in [
        TabKind::Session("s-1".to_string()),
        TabKind::Scene(tabs::SceneId::Files),
        TabKind::Scene(tabs::SceneId::Git),
        TabKind::Scene(tabs::SceneId::Insights),
        TabKind::Scene(tabs::SceneId::Tasks),
    ] {
        let kind_label = format!("{kind:?}");
        tabs.open(kind.clone());
        let lit: Vec<&NavAction> = actions
            .iter()
            .filter(|candidate| candidate.is_active(&tabs))
            .collect();
        match kind {
            // Files and git stay workbench panels without a nav row;
            // insights has its own row since E4 batch ①.
            TabKind::Scene(tabs::SceneId::Files) | TabKind::Scene(tabs::SceneId::Git) => {
                assert!(lit.is_empty(), "tab {kind_label} must light no nav row");
            }
            _ => {
                assert_eq!(
                    lit.len(),
                    1,
                    "tab {kind_label} must light exactly one nav row"
                );
            }
        }
    }
    // Settings keeps its footer-button variant, no nav row.
    tabs.open(TabKind::Scene(tabs::SceneId::Settings));
    let active = actions
        .iter()
        .filter(|candidate| candidate.is_active(&tabs))
        .count();
    assert_eq!(active, 0, "the settings tab lights no nav row");
    // The resting empty state is chat.
    let empty = TabsState::default();
    assert!(NavAction::Chat.is_active(&empty));
}

/// E4 batch ①: the extensions scene degrades onto the branded empty card
/// and its single action jumps to the external-agent settings page; the
/// mini-app card stays actionless because no honest destination exists.
#[gpui_kit::test]
fn extension_scene_renders_the_empty_card_with_the_acp_action(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    install_shell_globals(cx);
    let (tx, rx) = mpsc::unbounded_channel();
    // The shell must be the window's ROOT view: `click` re-renders the
    // window pipeline, so an ad-hoc `draw` frame would be replaced and
    // the button would vanish from the observation set mid-click.
    let (shell, window) = cx.add_window_view(|window, cx| ShellWindow::new(tx, rx, window, cx));
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.apply_nav(NavAction::Extensions, cx));
    });
    window.update(|window, cx| window.render_frame(cx));
    let card = window
        .debug_bounds("brand-empty-card")
        .expect("the extensions scene lays out the empty card");
    let action = window
        .debug_bounds("extensions-open-acp")
        .expect("the empty card carries the ACP settings action");
    assert!(
        action.origin.x >= card.origin.x
            && action.origin.y >= card.origin.y
            && action.origin.x + action.size.width <= card.origin.x + card.size.width
            && action.origin.y + action.size.height <= card.origin.y + card.size.height,
        "the action lives inside the card: action {:?} card {:?}",
        action,
        card
    );

    // The action is the one honest jump: it opens the settings scene on
    // the external-agents page (the surface the scene copy names).
    window.update(|window, cx| window.click("extensions-open-acp", cx));
    window.run_until_parked();
    window.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.tabs.active_kind(),
            Some(&TabKind::Scene(tabs::SceneId::Settings)),
            "the action opens the settings scene"
        );
        assert_eq!(
            shell.settings_page,
            settings::SettingsEntry::ExternalAgents,
            "the action lands on the external-agents page"
        );
    });

    // The mini-app card is the same treatment minus the action lane.
    window.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.apply_nav(NavAction::MiniApps, cx));
    });
    window.update(|window, cx| window.render_frame(cx));
    assert!(
        window.debug_bounds("brand-empty-card").is_some(),
        "the mini-app scene lays out the empty card too"
    );
    assert!(
        window.debug_bounds("extensions-open-acp").is_none(),
        "the mini-app card renders no action"
    );
}
