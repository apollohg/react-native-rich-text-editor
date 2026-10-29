const NATIVE_PUBLICATION_OWNER: u64 = 20_001;
const NATIVE_PUBLICATION_EDITOR: &str = "20000";
const NATIVE_PUBLICATION_TEXT: &str = "x";
const NATIVE_PUBLICATION_FIRST_REQUEST: u64 = 2;
const NATIVE_PUBLICATION_ARTICLE_BLOCKS: usize = 48;
const NATIVE_PUBLICATION_RANGE_WIDTH: u32 = 8;
const NATIVE_PUBLICATION_PASTE_TEXT: &str = "Pasted marked editor prose 🦀 ";
const NATIVE_PUBLICATION_PASTE_REPEATS: usize = 32;
const NATIVE_PUBLICATION_REPLACEMENT_TEXT: &str = "replacement🦀";
const NATIVE_PUBLICATION_SELECTION_MOVES: u32 = 64;
const NATIVE_PUBLICATION_SELECTION_STRIDE: u32 = 37;

struct NativePublicationFixture {
    session: crate::session::EditorSession,
    epoch: String,
    next_request: u64,
}

fn native_publication_fixture(document: &str) -> NativePublicationFixture {
    let config = crate::session::EditorSessionConfig {
        schema_json: None,
        fragment_name: "prosemirror".into(),
        initialization: crate::session::EditorInitialization::Local {
            initial_content: crate::session::InitialContent::Empty,
        },
        initial_selection: crate::session::InitialSelection::Start,
        resource_limits: ResourceLimits::default(),
        editing_limits: crate::yrs_engine::EditingLimits::default(),
        collaboration_limits: crate::session::CollaborationLimits::default(),
        max_length: None,
        read_only: false,
        input_filter: None,
        allow_base64_images: false,
    };
    let mut session = crate::session::EditorSession::new(
        yrs_editing_fixture(document, EDITING_CURSOR_SCALAR, EDITING_CURSOR_SCALAR),
        crate::session::SessionPolicy::from_config(&config),
        crate::session::DocumentState::LocalReady,
        crate::session::CollaborationLimits::default(),
    )
    .expect("native publication benchmark session should initialize");
    let frame = crate::ffi_v2::native_frame::build_native_frame(
        &mut session,
        NATIVE_PUBLICATION_EDITOR,
        Some(NATIVE_PUBLICATION_OWNER),
        None,
    )
    .expect("native publication benchmark should publish its first frame");
    NativePublicationFixture {
        session,
        epoch: frame
            .position_epoch
            .expect("an owned native frame publishes its position epoch"),
        next_request: NATIVE_PUBLICATION_FIRST_REQUEST,
    }
}

fn native_publication_insert(fixture: &mut NativePublicationFixture, keystroke: usize) {
    let caret = EDITING_CURSOR_SCALAR + keystroke as u32;
    native_publication_intent(
        fixture,
        json!({
            "type": "insertText", "anchor": caret, "head": caret, "text": NATIVE_PUBLICATION_TEXT
        }),
        true,
    );
}

fn native_publication_intent(
    fixture: &mut NativePublicationFixture,
    intent: Value,
    changes_document: bool,
) -> Value {
    let envelope = json!({
        "version": crate::native_transaction_bridge::NATIVE_BRIDGE_ENVELOPE_VERSION,
        "requestId": fixture.next_request.to_string(),
        "ownerId": NATIVE_PUBLICATION_OWNER.to_string(),
        "positionEpoch": fixture.epoch,
        "intent": intent,
    })
    .to_string();
    let outcome =
        crate::native_transaction_bridge::NativeTransactionBridge::new(&mut fixture.session)
            .submit_native_intent(&envelope)
            .expect("native publication intent should apply");
    let outcome: Value = serde_json::from_str(&outcome).unwrap();
    assert_eq!(
        outcome["documentChanged"], changes_document,
        "intent outcome: {outcome}"
    );
    fixture.next_request += 1;
    outcome
}

fn native_publication_frame(fixture: &mut NativePublicationFixture) -> Value {
    let frame = crate::ffi_v2::native_frame::build_native_frame(
        &mut fixture.session,
        NATIVE_PUBLICATION_EDITOR,
        Some(NATIVE_PUBLICATION_OWNER),
        None,
    )
    .expect("native publication benchmark frame should publish");
    let snapshot: Value = serde_json::from_str(&frame.snapshot_json)
        .expect("native publication frame should contain JSON");
    fixture.epoch = frame
        .position_epoch
        .expect("an owned native frame publishes its position epoch");
    snapshot
}

fn assert_native_publication(
    fixture: &NativePublicationFixture,
    snapshot: &Value,
    expected_document: &Value,
    revisions: usize,
    anchor: u32,
    head: u32,
) {
    assert_eq!(
        fixture.session.engine.document_json().as_ref(),
        Some(expected_document)
    );
    assert_eq!(fixture.session.engine.revision(), 1 + revisions as u64);
    assert_eq!(
        snapshot["documentVersion"],
        (1 + revisions as u64).to_string()
    );
    assert_eq!(snapshot["selection"]["headScalar"], head);
    assert_eq!(snapshot["selection"]["anchorScalar"], anchor);
    assert_eq!(snapshot["historyState"]["canUndo"], revisions > 0);
    assert_eq!(snapshot["renderBlocks"], Value::Null);
    assert_eq!(
        fixture
            .session
            .native_render_cursor(NATIVE_PUBLICATION_OWNER)
            .unwrap()
            .root_projection
            .as_slice(),
        crate::ffi_v2::render::root_projection(&fixture.session.engine, None)
            .unwrap()
            .into_retained()
            .as_slice(),
        "native publication cursor must match the complete current render projection"
    );
}

fn push_native_publication_cases(
    results: &mut Vec<BenchResult>,
    options: &BenchOptions,
    profile: BenchProfile,
) {
    const CASES: [(&str, &str, &str, &str, &str, &str, &str, &str, &str, &str, usize); 4] = [
        (
            "native.frame.same_revision.article.1x",
            "native.frame.after_insert.article.1x",
            "native.intent.typing_burst.article.1x",
            "native.intent.selection.article.1x",
            "native.intent.selection_state.article.1x",
            "native.intent.selection_moves.article.1x",
            "native.intent.format.article.1x",
            "native.intent.paste.article.1x",
            "native.intent.replace_selection.article.1x",
            "native.document.replace.article.1x",
            1,
        ),
        (
            "native.frame.same_revision.article.2x",
            "native.frame.after_insert.article.2x",
            "native.intent.typing_burst.article.2x",
            "native.intent.selection.article.2x",
            "native.intent.selection_state.article.2x",
            "native.intent.selection_moves.article.2x",
            "native.intent.format.article.2x",
            "native.intent.paste.article.2x",
            "native.intent.replace_selection.article.2x",
            "native.document.replace.article.2x",
            2,
        ),
        (
            "native.frame.same_revision.article.8x",
            "native.frame.after_insert.article.8x",
            "native.intent.typing_burst.article.8x",
            "native.intent.selection.article.8x",
            "native.intent.selection_state.article.8x",
            "native.intent.selection_moves.article.8x",
            "native.intent.format.article.8x",
            "native.intent.paste.article.8x",
            "native.intent.replace_selection.article.8x",
            "native.document.replace.article.8x",
            8,
        ),
        (
            "native.frame.same_revision.article.32x",
            "native.frame.after_insert.article.32x",
            "native.intent.typing_burst.article.32x",
            "native.intent.selection.article.32x",
            "native.intent.selection_state.article.32x",
            "native.intent.selection_moves.article.32x",
            "native.intent.format.article.32x",
            "native.intent.paste.article.32x",
            "native.intent.replace_selection.article.32x",
            "native.document.replace.article.32x",
            32,
        ),
    ];
    for (
        same_revision_name,
        after_insert_name,
        typing_name,
        selection_name,
        selection_state_name,
        selection_moves_name,
        format_name,
        paste_name,
        replace_selection_name,
        replace_document_name,
        multiplier,
    ) in CASES
    {
        let document = OnceLock::new();
        let document = || {
            document.get_or_init(|| {
                build_article_document(
                    NATIVE_PUBLICATION_ARTICLE_BLOCKS * multiplier,
                    profile.paragraph_chars,
                )
            })
        };
        let encoded = OnceLock::new();
        let encoded = || encoded.get_or_init(|| serde_json::to_string(document()).unwrap());
        let unchanged = OnceLock::new();
        let unchanged = || unchanged.get_or_init(|| build_yrs_expected_document(document()));
        let inserted = OnceLock::new();
        let inserted = || {
            inserted.get_or_init(|| {
                build_yrs_expected_document(&build_pure_insert_document(
                    document(),
                    NATIVE_PUBLICATION_TEXT,
                    1,
                ))
            })
        };
        let typed = OnceLock::new();
        let typed = || {
            typed.get_or_init(|| {
                build_yrs_expected_document(&build_pure_insert_document(
                    document(),
                    NATIVE_PUBLICATION_TEXT,
                    EDITING_TYPING_BURST,
                ))
            })
        };
        push_case!(
            results,
            options,
            verified_bench_case(
                same_revision_name,
                "native-publication",
                profile.iterations,
                profile.warmup_iterations,
                1,
                || (native_publication_fixture(encoded()), unchanged()),
                |(fixture, _)| black_box(native_publication_frame(fixture)),
                |(fixture, expected), snapshot| assert_native_publication(
                    fixture,
                    snapshot,
                    expected,
                    0,
                    EDITING_CURSOR_SCALAR,
                    EDITING_CURSOR_SCALAR
                ),
            ),
        );
        push_case!(
            results,
            options,
            verified_bench_case(
                after_insert_name,
                "native-publication",
                profile.iterations,
                profile.warmup_iterations,
                1,
                || {
                    let mut fixture = native_publication_fixture(encoded());
                    native_publication_insert(&mut fixture, 0);
                    (fixture, inserted())
                },
                |(fixture, _)| black_box(native_publication_frame(fixture)),
                |(fixture, expected), snapshot| assert_native_publication(
                    fixture,
                    snapshot,
                    expected,
                    1,
                    EDITING_CURSOR_SCALAR + 1,
                    EDITING_CURSOR_SCALAR + 1
                ),
            ),
        );
        push_case!(
            results,
            options,
            verified_bench_case(
                typing_name,
                "native-publication",
                profile.iterations,
                profile.warmup_iterations,
                EDITING_TYPING_BURST,
                || (native_publication_fixture(encoded()), typed()),
                |(fixture, _)| {
                    let mut final_snapshot = None;
                    for keystroke in 0..EDITING_TYPING_BURST {
                        native_publication_insert(fixture, keystroke);
                        final_snapshot = Some(black_box(native_publication_frame(fixture)));
                    }
                    final_snapshot.unwrap()
                },
                |(fixture, expected), snapshot| assert_native_publication(
                    fixture,
                    snapshot,
                    expected,
                    EDITING_TYPING_BURST,
                    EDITING_CURSOR_SCALAR + EDITING_TYPING_BURST as u32,
                    EDITING_CURSOR_SCALAR + EDITING_TYPING_BURST as u32
                ),
            ),
        );
        let range_head = EDITING_CURSOR_SCALAR + NATIVE_PUBLICATION_RANGE_WIDTH;
        let formatted = OnceLock::new();
        let formatted = || {
            formatted.get_or_init(|| {
                build_yrs_expected_document(&build_pure_bold_document(
                    document(),
                    EDITING_CURSOR_SCALAR,
                    range_head,
                ))
            })
        };
        let paste_text = NATIVE_PUBLICATION_PASTE_TEXT.repeat(NATIVE_PUBLICATION_PASTE_REPEATS);
        let pasted = OnceLock::new();
        let pasted = || {
            pasted.get_or_init(|| {
                build_yrs_expected_document(&build_pure_insert_document(document(), &paste_text, 1))
            })
        };
        let replaced = OnceLock::new();
        let replaced = || {
            replaced.get_or_init(|| {
                build_yrs_expected_document(&build_pure_replace_document(
                    document(),
                    NATIVE_PUBLICATION_REPLACEMENT_TEXT,
                    EDITING_CURSOR_SCALAR,
                    range_head,
                ))
            })
        };
        for (name, intent, expected, revisions, anchor, head) in [
            (
                selection_name,
                json!({"type":"setSelection","anchor":EDITING_CURSOR_SCALAR,"head":range_head}),
                unchanged(),
                0,
                EDITING_CURSOR_SCALAR,
                range_head,
            ),
            (
                format_name,
                json!({"type":"command","anchor":EDITING_CURSOR_SCALAR,"head":range_head,"command":{"type":"toggleMark","markType":"bold"}}),
                formatted(),
                1,
                EDITING_CURSOR_SCALAR,
                range_head,
            ),
            (
                paste_name,
                json!({"type":"command","anchor":EDITING_CURSOR_SCALAR,"head":EDITING_CURSOR_SCALAR,"command":{"type":"paste","text":paste_text,"plainText":true}}),
                pasted(),
                1,
                EDITING_CURSOR_SCALAR + paste_text.chars().count() as u32,
                EDITING_CURSOR_SCALAR + paste_text.chars().count() as u32,
            ),
            (
                replace_selection_name,
                json!({"type":"replaceSelectionText","anchor":EDITING_CURSOR_SCALAR,"head":range_head,"text":NATIVE_PUBLICATION_REPLACEMENT_TEXT}),
                replaced(),
                1,
                EDITING_CURSOR_SCALAR + NATIVE_PUBLICATION_REPLACEMENT_TEXT.chars().count() as u32,
                EDITING_CURSOR_SCALAR + NATIVE_PUBLICATION_REPLACEMENT_TEXT.chars().count() as u32,
            ),
        ] {
            push_case!(
                results,
                options,
                verified_bench_case(
                    name,
                    "native-publication",
                    profile.iterations,
                    profile.warmup_iterations,
                    1,
                    || (native_publication_fixture(encoded()), expected),
                    |(fixture, _)| {
                        native_publication_intent(fixture, intent.clone(), revisions > 0);
                        black_box(native_publication_frame(fixture))
                    },
                    |(fixture, expected), snapshot| assert_native_publication(
                        fixture, snapshot, expected, revisions, anchor, head
                    ),
                ),
            );
        }
        push_case!(
            results,
            options,
            verified_bench_case(
                selection_state_name,
                "native-publication",
                profile.iterations,
                profile.warmup_iterations,
                1,
                || native_publication_fixture(encoded()),
                |fixture| {
                    let mut outcome = native_publication_intent(
                        fixture,
                        json!({"type":"setSelection","anchor":EDITING_CURSOR_SCALAR,"head":range_head}),
                        false,
                    );
                    black_box(outcome["selectionState"].take())
                },
                |fixture, state| {
                    assert_eq!(state["selection"]["anchorScalar"], EDITING_CURSOR_SCALAR);
                    assert_eq!(state["selection"]["headScalar"], range_head);
                    assert_eq!(
                        state["documentVersion"],
                        fixture.session.engine.revision().to_string()
                    );
                    assert_eq!(state["historyState"]["canUndo"], false);
                },
            ),
        );
        let selection_move_caret = |step: u32| EDITING_CURSOR_SCALAR + step * NATIVE_PUBLICATION_SELECTION_STRIDE;
        push_case!(
            results,
            options,
            verified_bench_case(
                selection_moves_name,
                "native-publication",
                profile.iterations,
                profile.warmup_iterations,
                NATIVE_PUBLICATION_SELECTION_MOVES as usize,
                || native_publication_fixture(encoded()),
                |fixture| {
                    let mut last = Value::Null;
                    for step in 0..NATIVE_PUBLICATION_SELECTION_MOVES {
                        let caret = selection_move_caret(step);
                        let mut outcome = native_publication_intent(
                            fixture,
                            json!({"type":"setSelection","anchor":caret,"head":caret}),
                            false,
                        );
                        last = outcome["selectionState"].take();
                    }
                    black_box(last)
                },
                |_, state| {
                    let caret = selection_move_caret(NATIVE_PUBLICATION_SELECTION_MOVES - 1);
                    assert_eq!(state["selection"]["anchorScalar"], caret);
                    assert_eq!(state["selection"]["headScalar"], caret);
                },
            ),
        );
        push_case!(
            results,
            options,
            verified_bench_case(
                replace_document_name,
                "native-publication",
                profile.iterations,
                profile.warmup_iterations,
                1,
                || (
                    native_publication_fixture(encoded()),
                    inserted(),
                    serde_json::to_string(inserted()).unwrap()
                ),
                |(fixture, _, source)| {
                    let commit = fixture
                        .session
                        .replace_document_json(
                            fixture.next_request,
                            source,
                            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
                        )
                        .expect("native publication document replacement should apply");
                    assert!(commit.changed);
                    black_box(native_publication_frame(fixture))
                },
                |(fixture, expected, _), snapshot| assert_native_publication(
                    fixture, snapshot, expected, 1, 0, 0
                ),
            ),
        );
    }
}
