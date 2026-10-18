const TRANSCRIPT_REQUEST_ID: u64 = 70_090_000;
const TRANSCRIPT_PARAGRAPHS: usize = SHARED_SNAPSHOT_PARAGRAPHS;
const TRANSCRIPT_BURST_MILLIS: u64 = 10;
const TRANSCRIPT_BOUNDARY_MILLIS: u64 = 5_000;
const TRANSCRIPT_BURST: usize = 5;
const TRANSCRIPT_DELETES: usize = 3;
const TRANSCRIPT_REDOS: usize = 3;
const SNAPSHOT_EXCLUDING_METADATA_LIMIT: usize = 64 * 1024;
const TRANSCRIPT_PASTE: &str = "pasted words";

struct HistoryTranscript {
    engine: YrsDocumentEngine,
    peer: YrsDocumentEngine,
    clock: Arc<std::sync::atomic::AtomicU64>,
    request: u64,
    lines: Vec<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl HistoryTranscript {
    fn new(editing_limits: crate::yrs_engine::EditingLimits) -> Self {
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let history_clock = Arc::clone(&clock);
        let engine = YrsDocumentEngine::new_with_history_clock(
            transaction_engine_config(editing_limits),
            Arc::new(move || history_clock.load(std::sync::atomic::Ordering::SeqCst)),
        )
        .unwrap();
        let peer = YrsDocumentEngine::new(YrsEngineConfig {
            initialization_mode: crate::yrs_engine::InitializationMode::AwaitRemote,
            ..transaction_engine_config(crate::yrs_engine::EditingLimits::default())
        })
        .unwrap();
        Self {
            engine,
            peer,
            clock,
            request: TRANSCRIPT_REQUEST_ID,
            lines: Vec::new(),
        }
    }

    fn next_request(&mut self) -> u64 {
        self.request += 1;
        self.request
    }

    fn advance(&self, millis: u64) {
        self.clock
            .fetch_add(millis, std::sync::atomic::Ordering::SeqCst);
    }

    fn record(&mut self, label: &str) {
        let state = self.engine.encoded_state().unwrap();
        let delta = match self.peer.encoded_state() {
            Ok(peer_state) if !peer_state.is_empty() => yrs::diff_updates_v1(
                &state,
                &yrs::encode_state_vector_from_update_v1(&peer_state).unwrap(),
            )
            .unwrap(),
            _ => state.clone(),
        };
        let request = self.next_request();
        self.peer.apply_remote_update_v1(request, &delta).unwrap();
        assert_eq!(self.peer.document_json(), self.engine.document_json(), "{label}");
        self.lines.push(format!(
            "{label} state={} peer={} json={} selection={:?} undo={} redo={}",
            sha256_hex(&state),
            sha256_hex(&self.peer.encoded_state().unwrap()),
            sha256_hex(self.engine.document_json().unwrap().to_string().as_bytes()),
            self.engine.resolved_selection(),
            self.engine.can_undo(),
            self.engine.can_redo(),
        ));
    }

    fn select(&mut self, anchor: u32, head: u32) {
        let request = self.next_request();
        select_text(&mut self.engine, request, anchor, head);
    }

    fn command(&mut self, label: &str, command: TypedCommand) {
        let request = self.next_request();
        self.engine
            .apply_command(request, command)
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        self.record(label);
    }

    fn input(&mut self, label: &str, command: TypedCommand, caret: u32) {
        let request = self.next_request();
        let point = RevisionedPosition {
            offset: caret,
            kind: EditorOffsetKind::Scalar,
            affinity: Affinity::After,
        };
        self.engine
            .apply_command_at_selection_with_outbox(
                request,
                command,
                SelectionInput::Text {
                    anchor: point,
                    head: point,
                },
                TransactionOrigin::LocalInput,
                None,
            )
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        self.record(label);
    }

    fn coalescing_insert(&mut self, label: &str, caret: u32) {
        let request = self.next_request();
        let revision = self.engine.revision();
        self.engine
            .apply_typed_transaction(TypedTransaction {
                request_id: request,
                base_document_revision: revision,
                origin: TransactionOrigin::LocalInput,
                operations: vec![TypedOperation::InsertText {
                    at: RevisionedPosition {
                        offset: caret,
                        kind: EditorOffsetKind::Scalar,
                        affinity: Affinity::After,
                    },
                    text: "x".into(),
                    marks: Vec::new(),
                }],
                selection_intent: SelectionIntent::UseOperationResult,
                history_policy: HistoryPolicy::Auto,
            })
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        self.record(label);
    }

    fn undo(&mut self, label: &str) -> bool {
        let request = self.next_request();
        let undone = self.engine.undo(request).unwrap().is_some();
        self.record(label);
        undone
    }

    fn redo(&mut self, label: &str) -> bool {
        let request = self.next_request();
        let redone = self.engine.redo(request).unwrap().is_some();
        self.record(label);
        redone
    }
}

fn history_transcript(
    editing_limits: crate::yrs_engine::EditingLimits,
    retains_document_snapshots: bool,
) -> Vec<String> {
    let _clients = crate::test_support::deterministic_clients::DeterministicClients::new();
    let mut transcript = HistoryTranscript::new(editing_limits);
    transcript
        .engine
        .import_json(BLOCK_LOCAL_RICH_DOCUMENT, TransactionOrigin::DocumentImport)
        .unwrap();
    transcript.record("import");
    let request = transcript.next_request();
    transcript
        .engine
        .prepare_root_replacement_json(
            request,
            &loaded_article(TRANSCRIPT_PARAGRAPHS).to_string(),
            crate::yrs_engine::ReplacementHistory::UndoableBoundary,
        )
        .unwrap();
    transcript.record("load");
    let block = transcript
        .engine
        .derived_state
        .as_ref()
        .unwrap()
        .position_map
        .block(TRANSCRIPT_PARAGRAPHS / 2)
        .unwrap()
        .into_owned();
    let start = block.scalar_start + block.scalar_prefix_len;
    transcript.advance(TRANSCRIPT_BOUNDARY_MILLIS);
    let mut caret = start;
    for keystroke in 0..TRANSCRIPT_BURST {
        transcript.advance(TRANSCRIPT_BURST_MILLIS);
        transcript.coalescing_insert(&format!("burst {keystroke}"), caret);
        caret += 1;
    }
    transcript.advance(TRANSCRIPT_BOUNDARY_MILLIS);
    transcript.input(
        "bounded insert",
        TypedCommand::InsertText { text: "y".into() },
        caret,
    );
    caret += 1;
    transcript.advance(TRANSCRIPT_BOUNDARY_MILLIS);
    for delete in 0..TRANSCRIPT_DELETES {
        transcript.advance(TRANSCRIPT_BURST_MILLIS);
        transcript.input(
            &format!("delete {delete}"),
            TypedCommand::DeleteBackward,
            caret,
        );
        caret -= 1;
    }
    transcript.advance(TRANSCRIPT_BOUNDARY_MILLIS);
    transcript.select(start, start + 9);
    transcript.command(
        "bold",
        TypedCommand::ToggleMark {
            mark_type: "bold".into(),
        },
    );
    transcript.advance(TRANSCRIPT_BOUNDARY_MILLIS);
    transcript.select(start + 4, start + 4);
    transcript.command("split", TypedCommand::SplitBlock);
    transcript.advance(TRANSCRIPT_BOUNDARY_MILLIS);
    transcript.command(
        "paste",
        clipboard_paste(None, None, Some(TRANSCRIPT_PASTE.into()), true),
    );
    assert_eq!(
        transcript
            .engine
            .history
            .document_snapshot_identities_for_test()
            .iter()
            .flatten()
            .any(Option::is_some),
        retains_document_snapshots,
        "undo must exercise the intended restore path"
    );
    let mut undos = 0;
    while transcript.undo(&format!("undo {undos}")) {
        undos += 1;
    }
    for redo in 0..TRANSCRIPT_REDOS {
        assert!(transcript.redo(&format!("redo {redo}")));
    }
    transcript.advance(TRANSCRIPT_BOUNDARY_MILLIS);
    transcript.command(
        "edit after redo",
        TypedCommand::InsertText { text: "z".into() },
    );
    assert!(transcript.undo("undo after edit"));
    assert!(transcript.undo("second undo after edit"));
    assert!(transcript.redo("redo after edit"));
    transcript.lines
}

const DEFAULT_LIMIT_TRANSCRIPT: &[&str] = &[
    "import state=921be5c009e60fd7722b30b8f0ea0fc411c3b3c95e97d31a78b388b1614f7117 peer=921be5c009e60fd7722b30b8f0ea0fc411c3b3c95e97d31a78b388b1614f7117 json=ac3a5945522a3603da9e265f0dd20b249ac3e48150f547b86d8dfe9f84dcdc71 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=false redo=false",
    "load state=80560b69b0ffb0d68debf5c59873d3e639a4254ba606cb6895bc6e47052c8170 peer=80560b69b0ffb0d68debf5c59873d3e639a4254ba606cb6895bc6e47052c8170 json=3e0843ceb012b4b6077a9e3ac6b716a565fd45f9dd576637beff4b1f74236ad4 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=true redo=false",
    "burst 0 state=419d051d57fd35c49bb7a6a1816f0fe94336159b1b476ea7fcde68ef68561121 peer=419d051d57fd35c49bb7a6a1816f0fe94336159b1b476ea7fcde68ef68561121 json=4724c4d0f370eb27e50d1384b04ada148bea4b841d07ca664f3d770fad42770a selection=Some(Text { anchor: ResolvedPoint { document: 2198, scalar: 2185, utf16: 2197 }, head: ResolvedPoint { document: 2198, scalar: 2185, utf16: 2197 } }) undo=true redo=false",
    "burst 1 state=70d0d8cd023d7558554d3e85a008bf9064236272997802eef5d4721f9fa565c8 peer=70d0d8cd023d7558554d3e85a008bf9064236272997802eef5d4721f9fa565c8 json=fe4470945809bda6e1e337c8775018f509611b87bb0352c6aabed329c6485951 selection=Some(Text { anchor: ResolvedPoint { document: 2199, scalar: 2186, utf16: 2198 }, head: ResolvedPoint { document: 2199, scalar: 2186, utf16: 2198 } }) undo=true redo=false",
    "burst 2 state=9bfccca74531c388a003ed8e1d1f68ee90dfa9510478c287011e58efb7cf6660 peer=9bfccca74531c388a003ed8e1d1f68ee90dfa9510478c287011e58efb7cf6660 json=dbb76de6c14119ffef3cfaaf4dc51334b7534104e7b966d775224071cce7a050 selection=Some(Text { anchor: ResolvedPoint { document: 2200, scalar: 2187, utf16: 2199 }, head: ResolvedPoint { document: 2200, scalar: 2187, utf16: 2199 } }) undo=true redo=false",
    "burst 3 state=a4d40a0f97699a2515221d52ecc62d50fb2e9963eaf89fafa3b090f41307db62 peer=a4d40a0f97699a2515221d52ecc62d50fb2e9963eaf89fafa3b090f41307db62 json=4e3e0760c7956bdc50352ba44104df91d596dcea5c93297fcde2f8d0819b8f12 selection=Some(Text { anchor: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 }, head: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 } }) undo=true redo=false",
    "burst 4 state=6dec4b9f73ad8d52bc46ccf63b5549aea8882ec246367d8b581c61f2fcfa7b55 peer=6dec4b9f73ad8d52bc46ccf63b5549aea8882ec246367d8b581c61f2fcfa7b55 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=false",
    "bounded insert state=f6323b594962fc88cce448aa2c3b1f73d2a60e7689df9a4d528359c6a203b414 peer=f6323b594962fc88cce448aa2c3b1f73d2a60e7689df9a4d528359c6a203b414 json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=false",
    "delete 0 state=a0ff25cd8f608854ca73c037e3d48a01dad3d28666d03343026b41280af1319c peer=a0ff25cd8f608854ca73c037e3d48a01dad3d28666d03343026b41280af1319c json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=false",
    "delete 1 state=826b1ae883054dc98d6806d337ff2bd5b2106b438925dc73f8d250e2e4ac14b3 peer=826b1ae883054dc98d6806d337ff2bd5b2106b438925dc73f8d250e2e4ac14b3 json=4e3e0760c7956bdc50352ba44104df91d596dcea5c93297fcde2f8d0819b8f12 selection=Some(Text { anchor: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 }, head: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 } }) undo=true redo=false",
    "delete 2 state=d6c4ff007c47005200935e2c44e83c5642105803c28bc07da9a7958ef3bf1524 peer=d6c4ff007c47005200935e2c44e83c5642105803c28bc07da9a7958ef3bf1524 json=dbb76de6c14119ffef3cfaaf4dc51334b7534104e7b966d775224071cce7a050 selection=Some(Text { anchor: ResolvedPoint { document: 2200, scalar: 2187, utf16: 2199 }, head: ResolvedPoint { document: 2200, scalar: 2187, utf16: 2199 } }) undo=true redo=false",
    "bold state=9c6f16503ca11ade012b292f6707e9726a13332bbd046b77a8ca09427ec7c0b4 peer=9c6f16503ca11ade012b292f6707e9726a13332bbd046b77a8ca09427ec7c0b4 json=a06edd5fde723055d7081278af794434e00278057b8f6f52d54049fd977e2e5d selection=Some(Text { anchor: ResolvedPoint { document: 2197, scalar: 2184, utf16: 2196 }, head: ResolvedPoint { document: 2206, scalar: 2193, utf16: 2205 } }) undo=true redo=false",
    "split state=97fbe40d4648516837361617e60bed614ed1c1da91ba01bdf276bb417d915a1b peer=97fbe40d4648516837361617e60bed614ed1c1da91ba01bdf276bb417d915a1b json=46a63b1a5a68c48e0e1e21a0656f208a51c1b98f90ebd1996fa05da0599825c7 selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2203, scalar: 2189, utf16: 2201 } }) undo=true redo=false",
    "paste state=43dcd2ad6410eb43add12877f15f1c4aba3e469321367fe0cbb5ad38770192b0 peer=43dcd2ad6410eb43add12877f15f1c4aba3e469321367fe0cbb5ad38770192b0 json=bafbef5c0c7b43d127b8a26f5d9a3f4ed4cad1cb0d586b55c05effaeca7d57a3 selection=Some(Text { anchor: ResolvedPoint { document: 2215, scalar: 2201, utf16: 2213 }, head: ResolvedPoint { document: 2215, scalar: 2201, utf16: 2213 } }) undo=true redo=false",
    "undo 0 state=ff38355b6b962ab804780bb960f33312ba059fa7d836c86d5cae11201dae9db5 peer=ff38355b6b962ab804780bb960f33312ba059fa7d836c86d5cae11201dae9db5 json=46a63b1a5a68c48e0e1e21a0656f208a51c1b98f90ebd1996fa05da0599825c7 selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2203, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "undo 1 state=4b802fccb0bdfd8520989dda1174e85606ddfe3660ac2adbd27e38c9067b36e5 peer=4b802fccb0bdfd8520989dda1174e85606ddfe3660ac2adbd27e38c9067b36e5 json=a06edd5fde723055d7081278af794434e00278057b8f6f52d54049fd977e2e5d selection=Some(Text { anchor: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 }, head: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 } }) undo=true redo=true",
    "undo 2 state=e3ff5a365f24614517bd2d0e86839640009692d4eb6655f7903b87aa75fafcd9 peer=e3ff5a365f24614517bd2d0e86839640009692d4eb6655f7903b87aa75fafcd9 json=dbb76de6c14119ffef3cfaaf4dc51334b7534104e7b966d775224071cce7a050 selection=Some(Text { anchor: ResolvedPoint { document: 2197, scalar: 2184, utf16: 2196 }, head: ResolvedPoint { document: 2206, scalar: 2193, utf16: 2205 } }) undo=true redo=true",
    "undo 3 state=66d75ab7be1a65964555fe1ffefcd33f427f24c2c138dd339055bcd3cb1aa083 peer=a81a826ec7ae8ce6e262abddc8d965605204e772b122aa1f4e0c273989f26ce4 json=4e3e0760c7956bdc50352ba44104df91d596dcea5c93297fcde2f8d0819b8f12 selection=Some(Text { anchor: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 }, head: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 } }) undo=true redo=true",
    "undo 4 state=df6995ff391c57c837b6973e1fbd63289e3ec61c21ca3041653a3fdb42aa4305 peer=e6c8810ef6652231c89dca94ea3852f74d0b4efd66a59db38d74319703c908e1 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "undo 5 state=47c9256b54c29009b3830b6b80686a535eae84198b1507d65eb51d4c9150ee9c peer=47c9256b54c29009b3830b6b80686a535eae84198b1507d65eb51d4c9150ee9c json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=true",
    "undo 6 state=434aa3998a56254ff629aa07c157d5df4ea6f02d9dd269a20b2b1edbbee70830 peer=434aa3998a56254ff629aa07c157d5df4ea6f02d9dd269a20b2b1edbbee70830 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "undo 7 state=a21d6b5ca84306f7e6833049ada65026a3f5a996c9be4afbd2cacd2082337c9e peer=a21d6b5ca84306f7e6833049ada65026a3f5a996c9be4afbd2cacd2082337c9e json=3e0843ceb012b4b6077a9e3ac6b716a565fd45f9dd576637beff4b1f74236ad4 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=true redo=true",
    "undo 8 state=a0767010dd8fcc0bad6c4e79944c97b12bc360a3e14dec82261df985862828cf peer=a0767010dd8fcc0bad6c4e79944c97b12bc360a3e14dec82261df985862828cf json=ac3a5945522a3603da9e265f0dd20b249ac3e48150f547b86d8dfe9f84dcdc71 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=false redo=true",
    "undo 9 state=a0767010dd8fcc0bad6c4e79944c97b12bc360a3e14dec82261df985862828cf peer=a0767010dd8fcc0bad6c4e79944c97b12bc360a3e14dec82261df985862828cf json=ac3a5945522a3603da9e265f0dd20b249ac3e48150f547b86d8dfe9f84dcdc71 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=false redo=true",
    "redo 0 state=463a97ab00564acdf834144d6983721150c6afc352dfb295bf44890bd7ba1790 peer=463a97ab00564acdf834144d6983721150c6afc352dfb295bf44890bd7ba1790 json=3e0843ceb012b4b6077a9e3ac6b716a565fd45f9dd576637beff4b1f74236ad4 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=true redo=true",
    "redo 1 state=b520adbb53089d627978466eae71017347165f1af989deace4a15ab36f322893 peer=b520adbb53089d627978466eae71017347165f1af989deace4a15ab36f322893 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "redo 2 state=68d70d779d6b3689b88a6b75628579ac81c7143cbc854d9b8a94b2e2c1cc41cf peer=68d70d779d6b3689b88a6b75628579ac81c7143cbc854d9b8a94b2e2c1cc41cf json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=true",
    "edit after redo state=91ca53404eb00e3f355daf26945e572ec0d25d125d8063ff2e348087eba10087 peer=91ca53404eb00e3f355daf26945e572ec0d25d125d8063ff2e348087eba10087 json=f9bfbeacf1b123c58eaeee40e5fccdad51b2e1acdbd9095f681c512aa3ed30cd selection=Some(Text { anchor: ResolvedPoint { document: 2204, scalar: 2191, utf16: 2203 }, head: ResolvedPoint { document: 2204, scalar: 2191, utf16: 2203 } }) undo=true redo=false",
    "undo after edit state=95663eb1faf08e91a0a7204e79f4ab67423abc59bedd26f0d2c54f097f7c3c07 peer=95663eb1faf08e91a0a7204e79f4ab67423abc59bedd26f0d2c54f097f7c3c07 json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=true",
    "second undo after edit state=fc0981f03d5d3b879b3647f208692d1a49814d3cc42f1092ba21eb2d1425b738 peer=fc0981f03d5d3b879b3647f208692d1a49814d3cc42f1092ba21eb2d1425b738 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "redo after edit state=d33f3b3e2e8a2cf8b246c1a7ae3f7dbbf470ccda98b88933de86e110103948bd peer=4b95d4dd23613ae38beab91654c92e4c6a32f4f4740afbfaccffd387362cd909 json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=true",
];

const SNAPSHOT_EXCLUDING_TRANSCRIPT: &[&str] = &[
    "import state=921be5c009e60fd7722b30b8f0ea0fc411c3b3c95e97d31a78b388b1614f7117 peer=921be5c009e60fd7722b30b8f0ea0fc411c3b3c95e97d31a78b388b1614f7117 json=ac3a5945522a3603da9e265f0dd20b249ac3e48150f547b86d8dfe9f84dcdc71 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=false redo=false",
    "load state=80560b69b0ffb0d68debf5c59873d3e639a4254ba606cb6895bc6e47052c8170 peer=80560b69b0ffb0d68debf5c59873d3e639a4254ba606cb6895bc6e47052c8170 json=3e0843ceb012b4b6077a9e3ac6b716a565fd45f9dd576637beff4b1f74236ad4 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=true redo=false",
    "burst 0 state=419d051d57fd35c49bb7a6a1816f0fe94336159b1b476ea7fcde68ef68561121 peer=419d051d57fd35c49bb7a6a1816f0fe94336159b1b476ea7fcde68ef68561121 json=4724c4d0f370eb27e50d1384b04ada148bea4b841d07ca664f3d770fad42770a selection=Some(Text { anchor: ResolvedPoint { document: 2198, scalar: 2185, utf16: 2197 }, head: ResolvedPoint { document: 2198, scalar: 2185, utf16: 2197 } }) undo=true redo=false",
    "burst 1 state=70d0d8cd023d7558554d3e85a008bf9064236272997802eef5d4721f9fa565c8 peer=70d0d8cd023d7558554d3e85a008bf9064236272997802eef5d4721f9fa565c8 json=fe4470945809bda6e1e337c8775018f509611b87bb0352c6aabed329c6485951 selection=Some(Text { anchor: ResolvedPoint { document: 2199, scalar: 2186, utf16: 2198 }, head: ResolvedPoint { document: 2199, scalar: 2186, utf16: 2198 } }) undo=true redo=false",
    "burst 2 state=9bfccca74531c388a003ed8e1d1f68ee90dfa9510478c287011e58efb7cf6660 peer=9bfccca74531c388a003ed8e1d1f68ee90dfa9510478c287011e58efb7cf6660 json=dbb76de6c14119ffef3cfaaf4dc51334b7534104e7b966d775224071cce7a050 selection=Some(Text { anchor: ResolvedPoint { document: 2200, scalar: 2187, utf16: 2199 }, head: ResolvedPoint { document: 2200, scalar: 2187, utf16: 2199 } }) undo=true redo=false",
    "burst 3 state=a4d40a0f97699a2515221d52ecc62d50fb2e9963eaf89fafa3b090f41307db62 peer=a4d40a0f97699a2515221d52ecc62d50fb2e9963eaf89fafa3b090f41307db62 json=4e3e0760c7956bdc50352ba44104df91d596dcea5c93297fcde2f8d0819b8f12 selection=Some(Text { anchor: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 }, head: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 } }) undo=true redo=false",
    "burst 4 state=6dec4b9f73ad8d52bc46ccf63b5549aea8882ec246367d8b581c61f2fcfa7b55 peer=6dec4b9f73ad8d52bc46ccf63b5549aea8882ec246367d8b581c61f2fcfa7b55 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=false",
    "bounded insert state=f6323b594962fc88cce448aa2c3b1f73d2a60e7689df9a4d528359c6a203b414 peer=f6323b594962fc88cce448aa2c3b1f73d2a60e7689df9a4d528359c6a203b414 json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=false",
    "delete 0 state=a0ff25cd8f608854ca73c037e3d48a01dad3d28666d03343026b41280af1319c peer=a0ff25cd8f608854ca73c037e3d48a01dad3d28666d03343026b41280af1319c json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=false",
    "delete 1 state=826b1ae883054dc98d6806d337ff2bd5b2106b438925dc73f8d250e2e4ac14b3 peer=826b1ae883054dc98d6806d337ff2bd5b2106b438925dc73f8d250e2e4ac14b3 json=4e3e0760c7956bdc50352ba44104df91d596dcea5c93297fcde2f8d0819b8f12 selection=Some(Text { anchor: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 }, head: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 } }) undo=true redo=false",
    "delete 2 state=d6c4ff007c47005200935e2c44e83c5642105803c28bc07da9a7958ef3bf1524 peer=d6c4ff007c47005200935e2c44e83c5642105803c28bc07da9a7958ef3bf1524 json=dbb76de6c14119ffef3cfaaf4dc51334b7534104e7b966d775224071cce7a050 selection=Some(Text { anchor: ResolvedPoint { document: 2200, scalar: 2187, utf16: 2199 }, head: ResolvedPoint { document: 2200, scalar: 2187, utf16: 2199 } }) undo=true redo=false",
    "bold state=9c6f16503ca11ade012b292f6707e9726a13332bbd046b77a8ca09427ec7c0b4 peer=9c6f16503ca11ade012b292f6707e9726a13332bbd046b77a8ca09427ec7c0b4 json=a06edd5fde723055d7081278af794434e00278057b8f6f52d54049fd977e2e5d selection=Some(Text { anchor: ResolvedPoint { document: 2197, scalar: 2184, utf16: 2196 }, head: ResolvedPoint { document: 2206, scalar: 2193, utf16: 2205 } }) undo=true redo=false",
    "split state=97fbe40d4648516837361617e60bed614ed1c1da91ba01bdf276bb417d915a1b peer=97fbe40d4648516837361617e60bed614ed1c1da91ba01bdf276bb417d915a1b json=46a63b1a5a68c48e0e1e21a0656f208a51c1b98f90ebd1996fa05da0599825c7 selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2203, scalar: 2189, utf16: 2201 } }) undo=true redo=false",
    "paste state=43dcd2ad6410eb43add12877f15f1c4aba3e469321367fe0cbb5ad38770192b0 peer=43dcd2ad6410eb43add12877f15f1c4aba3e469321367fe0cbb5ad38770192b0 json=bafbef5c0c7b43d127b8a26f5d9a3f4ed4cad1cb0d586b55c05effaeca7d57a3 selection=Some(Text { anchor: ResolvedPoint { document: 2215, scalar: 2201, utf16: 2213 }, head: ResolvedPoint { document: 2215, scalar: 2201, utf16: 2213 } }) undo=true redo=false",
    "undo 0 state=ff38355b6b962ab804780bb960f33312ba059fa7d836c86d5cae11201dae9db5 peer=ff38355b6b962ab804780bb960f33312ba059fa7d836c86d5cae11201dae9db5 json=46a63b1a5a68c48e0e1e21a0656f208a51c1b98f90ebd1996fa05da0599825c7 selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2203, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "undo 1 state=4b802fccb0bdfd8520989dda1174e85606ddfe3660ac2adbd27e38c9067b36e5 peer=4b802fccb0bdfd8520989dda1174e85606ddfe3660ac2adbd27e38c9067b36e5 json=a06edd5fde723055d7081278af794434e00278057b8f6f52d54049fd977e2e5d selection=Some(Text { anchor: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 }, head: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 } }) undo=true redo=true",
    "undo 2 state=e3ff5a365f24614517bd2d0e86839640009692d4eb6655f7903b87aa75fafcd9 peer=e3ff5a365f24614517bd2d0e86839640009692d4eb6655f7903b87aa75fafcd9 json=dbb76de6c14119ffef3cfaaf4dc51334b7534104e7b966d775224071cce7a050 selection=Some(Text { anchor: ResolvedPoint { document: 2197, scalar: 2184, utf16: 2196 }, head: ResolvedPoint { document: 2206, scalar: 2193, utf16: 2205 } }) undo=true redo=true",
    "undo 3 state=66d75ab7be1a65964555fe1ffefcd33f427f24c2c138dd339055bcd3cb1aa083 peer=a81a826ec7ae8ce6e262abddc8d965605204e772b122aa1f4e0c273989f26ce4 json=4e3e0760c7956bdc50352ba44104df91d596dcea5c93297fcde2f8d0819b8f12 selection=Some(Text { anchor: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 }, head: ResolvedPoint { document: 2201, scalar: 2188, utf16: 2200 } }) undo=true redo=true",
    "undo 4 state=df6995ff391c57c837b6973e1fbd63289e3ec61c21ca3041653a3fdb42aa4305 peer=e6c8810ef6652231c89dca94ea3852f74d0b4efd66a59db38d74319703c908e1 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "undo 5 state=47c9256b54c29009b3830b6b80686a535eae84198b1507d65eb51d4c9150ee9c peer=47c9256b54c29009b3830b6b80686a535eae84198b1507d65eb51d4c9150ee9c json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=true",
    "undo 6 state=434aa3998a56254ff629aa07c157d5df4ea6f02d9dd269a20b2b1edbbee70830 peer=434aa3998a56254ff629aa07c157d5df4ea6f02d9dd269a20b2b1edbbee70830 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "undo 7 state=a21d6b5ca84306f7e6833049ada65026a3f5a996c9be4afbd2cacd2082337c9e peer=a21d6b5ca84306f7e6833049ada65026a3f5a996c9be4afbd2cacd2082337c9e json=3e0843ceb012b4b6077a9e3ac6b716a565fd45f9dd576637beff4b1f74236ad4 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=true redo=true",
    "undo 8 state=a0767010dd8fcc0bad6c4e79944c97b12bc360a3e14dec82261df985862828cf peer=a0767010dd8fcc0bad6c4e79944c97b12bc360a3e14dec82261df985862828cf json=ac3a5945522a3603da9e265f0dd20b249ac3e48150f547b86d8dfe9f84dcdc71 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=false redo=true",
    "undo 9 state=a0767010dd8fcc0bad6c4e79944c97b12bc360a3e14dec82261df985862828cf peer=a0767010dd8fcc0bad6c4e79944c97b12bc360a3e14dec82261df985862828cf json=ac3a5945522a3603da9e265f0dd20b249ac3e48150f547b86d8dfe9f84dcdc71 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=false redo=true",
    "redo 0 state=463a97ab00564acdf834144d6983721150c6afc352dfb295bf44890bd7ba1790 peer=463a97ab00564acdf834144d6983721150c6afc352dfb295bf44890bd7ba1790 json=3e0843ceb012b4b6077a9e3ac6b716a565fd45f9dd576637beff4b1f74236ad4 selection=Some(Text { anchor: ResolvedPoint { document: 1, scalar: 0, utf16: 0 }, head: ResolvedPoint { document: 1, scalar: 0, utf16: 0 } }) undo=true redo=true",
    "redo 1 state=b520adbb53089d627978466eae71017347165f1af989deace4a15ab36f322893 peer=b520adbb53089d627978466eae71017347165f1af989deace4a15ab36f322893 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "redo 2 state=68d70d779d6b3689b88a6b75628579ac81c7143cbc854d9b8a94b2e2c1cc41cf peer=68d70d779d6b3689b88a6b75628579ac81c7143cbc854d9b8a94b2e2c1cc41cf json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=true",
    "edit after redo state=91ca53404eb00e3f355daf26945e572ec0d25d125d8063ff2e348087eba10087 peer=91ca53404eb00e3f355daf26945e572ec0d25d125d8063ff2e348087eba10087 json=f9bfbeacf1b123c58eaeee40e5fccdad51b2e1acdbd9095f681c512aa3ed30cd selection=Some(Text { anchor: ResolvedPoint { document: 2204, scalar: 2191, utf16: 2203 }, head: ResolvedPoint { document: 2204, scalar: 2191, utf16: 2203 } }) undo=true redo=false",
    "undo after edit state=95663eb1faf08e91a0a7204e79f4ab67423abc59bedd26f0d2c54f097f7c3c07 peer=95663eb1faf08e91a0a7204e79f4ab67423abc59bedd26f0d2c54f097f7c3c07 json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=true",
    "second undo after edit state=fc0981f03d5d3b879b3647f208692d1a49814d3cc42f1092ba21eb2d1425b738 peer=fc0981f03d5d3b879b3647f208692d1a49814d3cc42f1092ba21eb2d1425b738 json=e2bd7d95272c5ed7dcb86e613902b2e8d57cdad819051dbf24303e96e24409f9 selection=Some(Text { anchor: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 }, head: ResolvedPoint { document: 2202, scalar: 2189, utf16: 2201 } }) undo=true redo=true",
    "redo after edit state=d33f3b3e2e8a2cf8b246c1a7ae3f7dbbf470ccda98b88933de86e110103948bd peer=4b95d4dd23613ae38beab91654c92e4c6a32f4f4740afbfaccffd387362cd909 json=734ddb589b4e124e702dd0b08ad291577f6904d74ef3568aaa419064c03b652f selection=Some(Text { anchor: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 }, head: ResolvedPoint { document: 2203, scalar: 2190, utf16: 2202 } }) undo=true redo=true",
];

#[test]
fn history_replay_and_remote_replay_transcripts_match_the_recorded_implementation() {
    for (name, limits, retains_document_snapshots, expected) in [
        (
            "default limits",
            crate::yrs_engine::EditingLimits::default(),
            true,
            DEFAULT_LIMIT_TRANSCRIPT,
        ),
        (
            "document snapshots excluded",
            crate::yrs_engine::EditingLimits {
                max_derived_output_bytes: SNAPSHOT_EXCLUDING_METADATA_LIMIT,
                ..crate::yrs_engine::EditingLimits::default()
            },
            false,
            SNAPSHOT_EXCLUDING_TRANSCRIPT,
        ),
    ] {
        let actual = history_transcript(limits, retains_document_snapshots);
        assert_eq!(
            actual,
            expected.iter().map(|line| (*line).to_owned()).collect::<Vec<_>>(),
            "{name}: transcript changed:\n{}",
            actual.join("\n")
        );
    }
}
