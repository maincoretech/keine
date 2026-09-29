# T21 — Card Editor source-preserving authoring

**Execution:** `implementation`

**Status:** Approved Block/Inspector implementation complete; remaining macOS interaction acceptance belongs to the author; blip explicitly excluded

## Goal

Make the `.shou` Blocks view a complete source-preserving writing surface before File import and
Asset management are introduced. Text remains the authority; Blocks, Inspector, and the Picker
operate on bounded source ranges rather than a second document model.

## Current product direction (2026-09-28)

The requested baseline is LetsGal's complete Block and Inspector UX with Kēne's palette.
Earlier representative QA does not establish that baseline. Preserve the accepted `.shou`
syntax and its source authority while matching the reference interactions.

```text
Reference
├── Installed LetsGal Studio 2.0.0: observed directly with Computer Use
├── Actual protected frontend: decoded in an authorized temporary analysis process
├── 40 JS/CSS/HTML files: /private/tmp/keine-studio-reference/renderer
├── Main UI: assets/main-D5mLo2Vz.js
└── Main styles: assets/main-CJx_hRCk.css
Block
├── Left-aligned content up to 700 px; minimum 16 px line-number gutter, 8 px right padding
├── Right-aligned row numbers with 4 px card gap; no extra base indentation
├── 40 px top / 120 px bottom padding
├── Narration body without a generic Text tag; dialogue speaker above body
├── 24 px combined icon/type tags, line numbers and permanent in-card drag grip
├── No hover insertion arrows beside cards; insertion remains in the node context menu
├── Restored original grip drag initiation and before/after drop placement; cards have no outline
├── Nested Blocks retain indentation without vertical guide lines
├── Right overview paints Scene/Block structure, visible range, selection and diagnostic markers
├── Overview click, drag and wheel navigate without changing Block selection or seeking Engine
├── Type badges and overview share one semantic palette; card surfaces retain the workbench colors
├── Right-click: run to here, copy, duplicate, cut, paste, select all, insert, move and delete
├── Wait: numeric input, hover presets and Wait for input mode on every row
├── Resource command: shared source-backed Picker on every row; thumbnails, paths, search and manager
├── Duplicate creates fresh implicit identities; paste inserts before the current Block and selects it
├── Tab closes an open Picker; vertical arrows wrap, horizontal arrows change nonempty categories
├── Disable / Enable uses the approved block-comment marker; disabled nodes remain visible and dimmed
└── Text owns directly following native Ending commands for movement, clipboard and deletion
Inspector
├── NumberSliderField arrangement: input, unit, slider; source units remain unchanged
├── 76 effect numeric controls: ranges read from actual Studio detail controls
├── Effect list with one active detail pane and include/remove toggles; native model owns neutral defaults
├── Properties / Transform / Layout / Playback / Timing groups shown directly
├── Speaking / Other characters / Narration style groups
├── Searchable asset, enum, speaker and voice selectors; boolean switches
├── Enter / blur commit; invalid input restores the original display value
├── Source entity reuse preserves focus and drafts across sibling property edits
├── Position pad: 16:9 grid, atomic X/Y writeback while dragging, 25% Shift snapping
├── Stage timeline shows track point markers; clicking selects the source Key in both views
├── Stage Replay uses the existing native Engine seek/show path
├── Wait Inspector: duration, always-visible presets, Wait for input and Replay
├── Stage / Track / Key: direct command-specific forms; Stage duration precedes animation ID
├── Shake axes / falloff and sprite position: segmented controls
├── Number sliders write back continuously during dragging
├── Shared resource Picker: keyboard wrap/Enter, outside/Escape/Tab/scroll/resize dismissal
├── Dedicated Picker action context keeps Enter from creating a Text draft beneath the popup
├── Audio audition: Engine decoder, transient window-free host, stop/switch/EOF cleanup
├── Resource manager entry; picking an asset also updates Preview while retaining Block selection
├── Track image controls apply only to character tracks; primary resources have one display owner
├── Text Ending: Keep dialogue / Keep characters, explicit hide target and optional transition
├── Existing text.retract: compact source → prefix row, multiline text fields and Replay
└── Track property choices share the loader's actual stage-property inventory
Acceptance
├── Existing workspace gates only; no additional test project
├── Untested implemented interactions belong to the author, as requested
└── Unimplemented reference interactions remain development work, not acceptance
```

## Implemented boundary

- One document continuously renders all of its Scenes as animated, collapsible sections.
- Enter continues into a new Text row, committing an empty source-backed block when needed;
  Shift+Enter inserts a line break within the current block. New rows also work inside an empty Scene.
- Text bodies edit inline. Speaker, Voice, and Stable ID are edited in Inspector and explicit Stable
  IDs remain project-unique.
- Tab opens the full searchable Block Picker. Normal browsing uses a compact wrapping two-column
  layout, falling back to one column when narrow; customization keeps one row per item for its
  controls. Every Picker icon is included in the Editor asset source. Arrow/Enter and mouse
  selection work; Tab closes the open Picker; favorites, category/item order, and hidden browse items persist as global Editor
  preferences. Hidden items remain reachable by search.
- Click, platform-modifier, Shift range selection, empty-space clearing, deletion, complete-node
  copy/paste, keyboard movement, and direct drag reorder use one selection model. Discrete sibling
  selections move as one group in source order; structurally unsafe moves fail closed.
- Text-to-Blocks selection resolves the deepest source node and scrolls it into view. Blocks-to-Text
  navigation restores the exact source line and column.
- Unknown source remains a compact read-only row at its real source position and navigates to Text.
- Choice, If / Else if / Else, and Loop use lightweight headers and indentation.
  Trailing Voice IDs remain part of their Text Block instead of becoming false Blocks.
- Non-Text cards show their command type, primary value, and a few source-backed parameter values.
  Stage animation keeps Track and Key indented without guide lines; Key is a compact single-line card.
  Inspector presents bounded source fields with clear inputs and useful choices for booleans,
  easing, camera targets, and common times. Unset optional fields are displayed in their property
  group and remain unwritten until edited. Multi-selection reports common versus mixed properties.
- Blocks keeps Scene creation and per-Scene rename/delete/move in the current document, using a
  compact header menu instead of a second Scene navigation page. Rename updates exact `goto` and
  `call` targets across indexed `.shou` sources; deletion confirms unresolved references.
- Supported native command rows use command-specific icons and short summaries. Inspector edits
  source-bounded parameters, including optional named arguments; unknown calls remain read-only.

## Performance and persistence

- No new dependency, editor audio decoder, duplicated source tree, or continuous animation loop was
  added. Audition uses the existing Preview control worker and a transient Engine control host;
  it does not create a Bevy app, native game window, or execute Program.
- Picker preferences use one small schema-versioned global app-data file written by atomic replace;
  they never enter project files.
- Block and Inspector edits target bounded source ranges. Scene rename preflights related
  documents, then uses their existing undo, recovery, save, diagnostics, and Preview paths.

## Validation

```text
cargo fmt --all --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo validate tests/fixtures/native-smoke
```

Historical macOS checks covered nested Stage Track/Key cards and Inspector time/value
writeback. Those observations predate the current replication request. Current code must pass
the gates above; full LetsGal interaction parity has not been accepted.

The 2026-09-28 increment passed format, workspace check, strict Clippy, workspace tests, and
native-smoke validation. Gates were rerun against current source after the UI edits. The existing macOS
application and `/private/tmp/keine-v11-ui-qa` were reused; no new test project was added.

```text
Observed in the running macOS Editor
├── One-line Key cards and external hover grip/insert controls remain visible inside an open Scene
├── Inspector numeric writeback on blur; invalid input restores the original value
├── Numeric fields use one padding owner and right alignment; 2000 ms displays without clipping
├── Timeline marker 0: selects Key 0, value 0, time 0
├── Timeline marker 2: selects Key 2, value 20, time 2000 ms
├── Block context menu: Run to here / Duplicate, icons and operation separators; Escape dismisses it
├── Duplicate: inserted immediately after the source Block, selected, then undone
├── Cmd+C / Cmd+V: pasted before Camera shake, selected the inserted Block, then undone
├── Stage Inspector: Replay entry visible; additional repeats default matches engine zero
├── Position drag: X 20 / Y 0 → X 486 / Y -261, with matching Block and Inspector values
├── Wait Inspector: 500 ms preset, then Wait for input writes the existing wait.advance() command
├── Figure resource Picker: search BG00233, Down/Enter selects BG00233SL.jpg and closes the popup
├── Resource selection writes source, retains Figure selection and updates the upper-right image Preview
├── Figure row shows one resource control and Slot hero, without repeated Arg/resource chips
├── Camera Track shows camera → x and Target / Property / Muted, without an image control
├── Existing real-project bgm/11.mp3 audition: Play → Stop → Play, with no path error or gameplay window
├── Stage Replay launches the paired native Engine; detailed animation appearance remains unaccepted
├── Selecting the dialogue displays P7 source edit save test in the native Engine
├── Clicking inside the Engine advances execution; Editor follows to Camera move
└── QA source restored exactly and saved after temporary edits
Remaining acceptance
├── All resource types, audition sound quality and all-row Wait edge cases
├── Effect inclusion/removal and Shift snapping
├── Multi-selection/explicit-ID clipboard edge cases and Stage Replay animation appearance
└── Full LetsGal parity remains open; these observations are limited incremental evidence
```

The prior disabled/Ending increment had 110 Editor unit tests; two source-preservation tests cover disabled nodes and
Text Ending operations in the existing projection suite, without another test project.
The `@stable_id` boundary remains dialogue, narration and choice options, as required by v1.

```text
Current approved-source increment — running macOS Editor observations
├── Camera move context menu offers Disable, then Enable after disabling
├── Enable restores the original command and its editable Inspector fields
├── Text Keep dialogue off inserts text.box(visible: false, auto: true) beneath the Text
├── Explicit hero* target inserts hide(hero*) beneath the same Text and turns Keep characters off
├── Turning both Keep switches on removes those associated native commands
├── Track Property opens a searchable list sourced from the loader's stage-property inventory
├── Final rebuilt Editor: Text right-click displays only the complete-node Block menu
├── Final rebuilt Editor: disabled Camera move is dimmed with the normal command palette
├── Source Undo restores the entire disabled command and Save persists the original QA source
└── Temporary source changes were restored exactly and saved in the existing QA project
```

Text rows own their complete-node context menu. The inline Textarea provides an empty native
menu, so GPUI Component does not display its second OS input popup over the Block menu. This
uses the pinned component's `context_menu` builder and `NativeMenu::show` empty-menu guard.

Window pointer listeners are registered during GPUI paint, as required by its API contract.
Each pointer update reads the same command's current bounded fields because multiple events
may arrive before Inspector renders again; X/Y are applied in one source edit. Mouse-up commits
the endpoint and clears the drag state, including release outside the position pad.


The recovered reference removes the earlier source-location blocker. The installed application
was not modified; temporary integrity-cache changes and decoding stayed in the authorized
analysis process. No reference bundle, native decoder, or new package dependency enters Kēne.
This increment is still not full LetsGal UX parity. Current runtime observations and remaining
interaction acceptance are recorded separately from build/test gates.

## Approved native additions and exclusion

```text
Native additions — approved and implemented
├── Shake amplitude/frequency randomness: normalized 0–1 source; 0–100% Inspector
├── Independent per-field tween participation: tween: [field, ...]; ◆ / ◇ controls
├── Explicit empty list applies immediately; omission retains the existing command behavior
├── LetsGal tweenFields and both randomness units are preserved during compilation
└── Shared typed numeric inventory owns native validation, UI switches and runtime sampling
Excluded by author
└── Voice-blip presets/custom settings and batch application: explicitly do not add
Native boundary
├── Existing dotted .shou style remains the source authority
├── New Action / StageEvent variants are appended; old Postcard indices/layout remain stable
├── PostProcessAnimation and CameraShakeState are transient; Save v11 serialization is unchanged
└── The separate LetsGal performance designer is not the Stage Inspector itself
```


## Approved source representations — implemented (2026-09-28)

```text
Disabled Block
├── Uses the existing block comment syntax
│   └── /* disabled
│         camera.move(scene, x: 20),
│       */
├── Engine/CLI continue to ignore that comment
├── Editor recognizes the approved marker and keeps the Block visible, dimmed and recoverable
├── Disabled selection does not seek the Engine; Enable restores the preserved command
├── Enable, reorder, clipboard insertion and deletion preserve valid active statement separators
└── Unbalanced comment delimiters fail closed without escaping or rewriting authored strings
Text lifetime
├── Preserves hero: "Dialogue", voice and optional @stable_id metadata
├── Keeps text.box(visible: false, auto: true) / hide(hero*, transition: fade(200ms)) as native commands
├── Associates at most one of each directly following command at the same source depth
├── Inspector edits Ending controls; associated commands appear indented below their Text
├── Copy, reorder and deletion carry the associated commands with the owning Text
├── Blank hide target keeps characters; disabling Keep characters focuses an explicit target input
└── No implicit character prefix or new action/source grammar is invented
Approved camera additions
├── camera.shake(..., amplitude_randomness: 0.3, frequency_randomness: 0.2)
├── camera.move / camera.effect / camera.effect.v2(..., tween: [numeric_field, ...])
└── Voice-blip intentionally excluded by the author
```

Audition extends authoring protocol 6 → 7. Editor and Engine must be updated together; the
existing exact-version handshake rejects a mixed pair. Audio decoding remains owned by
`src/runtime/audio.rs`: canonical Opus uses the same mount-backed streaming decoder as gameplay;
compatibility formats use the existing feature-gated seekable decoder and 128 MiB bound.
Rodio 0.22.2's pinned primary source requires retaining both `MixerDeviceSink` and `Player`;
the authoring Session owns them and drops them on stop, replacement or disconnect. Its request
accepts project-relative file paths, resolves the last matching filesystem asset mount, and
passes its confined logical path to the runtime decoder. `ContentMount::contains_file` retains
file/symlink confinement; a project file outside declared asset mounts is rejected. The current
Editor file picker does not expose archive-only assets for audition. Existing process
handshake coverage now also checks empty audition state, stopping, and rejection of absolute
and parent-traversal paths without starting gameplay. No additional test project was created.

After the audition path fix, format, workspace check, strict Clippy, workspace tests and
native-smoke validation were rerun and passed. Root tests: 256 passed / 1 ignored; core: 102;
Editor: 108; loader: 156; existing authoring process tests: 5. No-audio and bundled-Opus feature
checks passed.
Runtime observations used the rebuilt Editor and protocol-7 Engine installed in the existing
macOS QA application directory. These checks do not close full reference parity or P7.

The approved disabled/Text Ending increment, including the two observed menu/opacity repairs,
passed every workspace gate and native-smoke validation against current source. Root tests:
256 passed / 1 ignored; core: 102; Editor: 110; loader: 156; authoring process: 5. The final
release Editor was rebuilt, installed in the existing QA application and signature-verified
before the final visual checks. No new test project or dependency was added.

## Existing native sentence-tail retraction (2026-09-28)

```text
Native authority
├── Existing text.retract(source: "...", keep: "...") → RetractDialogue
├── Empty source resolves from current dialogue; empty keep erases the entire line
├── Grapheme deletion, a fresh advance after completion and consecutive retractions remain Engine-owned
└── No change to Action schema, Save v11, typewriter timing or authoring protocol
Editor
├── One-line original → keep summary; line breaks display as ↵, without repeated Source / Keep chips
├── Full text / Keep prefix use retained multiline drafts and bounded quoted-string writeback
├── Enter applies, Shift+Enter inserts a line, blur applies; sibling edits preserve focus and drafts
├── Replay uses the existing native Engine seek/show path
└── Text Ending and clipboard operations leave later retraction steps independent
Evidence
├── Before repair: actual QA row repeated Source and displayed empty Source as raw syntax
├── Runtime Inspector checks: prefix commit, Shift+Enter draft, quotes, newline and a family emoji
├── Native Replay: first step retained 我当然; a new Engine click selected the second step retaining 我
├── Another new Engine click advanced to Camera move and Editor followed that source Block
├── Reopen checks found multiline initialization clipping; use the pinned control's set_value row initialization
├── Final rebuilt Editor: multiline Inspector fully visible on reopen; summary and empty fields checked
├── Existing source-safety tests extended with consecutive retractions, empty fields and Unicode/escape text
└── Existing QA source restored exactly and saved; no new project, dependency or engine acceptance hook
```

The final retraction increment passed `cargo fmt --all --check`, `cargo check --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and
`cargo validate tests/fixtures/native-smoke`. Root: 256 passed / 1 ignored; core: 102; Editor:
110; loader: 156; authoring process: 5. The final release Editor was installed in the existing
macOS QA application and observed after reopen. Existing Engine semantics are exercised by
their unchanged core tests; mid-animation save/resume was not additionally checked through GUI.

## Camera randomness and field tween increment (2026-09-28)

```text
Implementation
├── Shared CameraTweenField inventory covers transform/effect/V2 numeric properties
├── Inspector: per-field ◆ / ◇; bounded source writeback, hidden raw tween-list input
├── Inspector: amplitude/frequency randomness 0–100%, step 5%; source remains 0–1
├── Native lowering: one selected camera command → one SetCameraTween; no list retains old Action
├── Runtime: selected values tween, other values apply atomically at the start; empty list does not wait
├── Explicit selection interpolates V2 numeric construction fields as well as intensity/position
├── Shake: smooth deterministic amplitude/frequency jitter; zeros preserve legacy arithmetic and skip noise
├── LetsGal: masks preserve offsetX/offsetY/zoom and numeric effects; composite camera applies atomically
├── LetsGal: ordinary percentage fields and normalized Stage fields use their respective units
└── Voice-blip deliberately excluded by the author
Current-source gates
├── cargo fmt --all --check: passed
├── cargo check --workspace: passed
├── cargo clippy --workspace --all-targets -- -D warnings: passed
├── cargo test --workspace: passed; root 257 / 1 ignored, core 104, Editor 112, loader 158
├── Existing authoring process tests: 5 passed; official local LetsGal sample acceptance: passed
├── Sample retains 1020 original components; selected camera components combine into 1007 Actions
└── cargo validate tests/fixtures/native-smoke: passed, 0 warnings
```

Postcard 1.1.3's pinned `ser/serializer.rs` writes each enum variant index as a varint.
The new Action and StageEvent variants append after every existing variant; old payload structs
and their layouts stay unchanged. Runtime camera animations are skipped by State serialization,
so the extra transient selection/randomness state does not change Save v11. New selected Actions
also pass a Postcard roundtrip. No dependency, extra test project or GUI acceptance hook was added.

```text
Current macOS runtime observations — rebuilt Editor and Engine in the existing QA application directory
├── Copy hash checked before signing; both locally sealed bundles passed codesign --verify --deep --strict
├── Blur ◆ toggled off: source wrote tween: [x] without inserting an unwritten Blur value
├── Source with tween: [x] restored hollow states for the other numeric fields
├── Randomness numeric inputs: 30% / 20% wrote amplitude_randomness: 0.3 / frequency_randomness: 0.2
├── Saved, closed and reopened main.shou: both percentage values persisted in Inspector
├── Current release Engine validated that source: 5 Actions, 0 warnings; native Preview launched
├── Editor dialogue selection displayed its text in Engine; direct Engine click followed Camera move
├── Long shake parameters exposed a truncated-summary defect: target disappeared and duplicated as a chip
├── Final rebuilt Editor now reads the bounded positional field; target all displays once on reopen
├── Native Preview was stopped explicitly; QA source restored and compared byte-for-byte with its original
└── Motion appearance for every effect/target remains author acceptance; no new test project was created
```

The final display repair passed the full workspace gates again. The current Engine release includes
macOS native video; the final Editor release was rebuilt and installed after that observed repair.

## Block drag restoration (2026-09-28)

The author requested the original drag interaction after the external hover-only grip became
hard to use. The grip is again always visible inside each movable card, with the original GPUI
drag initiation and row selection behavior. The added left mouse-down propagation stop and
grip-click context menu were removed. GPUI 0.3.5 records its drag mouse-down in the bubble phase;
that event must reach its retained drag handler. Right-click continues to open the node menu.

```text
Restored Block interaction
├── In-card grip: always visible; original before/after drop slots and animated placeholders retained
├── Normal and selected cards: no outline; selection uses background fill
├── Running rebuilt macOS Editor: single camera node moved upward, then downward
├── Stage moved with its Track, both Keys and event; saved source preserved the subtree
├── Camera selection still updates Inspector; selected card has no outline
├── Existing QA source restored, saved and compared byte-for-byte with its original
└── Discrete multi-selection interaction remains author acceptance
```

Current source passed `cargo fmt --all --check`, `cargo check --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.
The Editor release was rebuilt and copied into the existing macOS QA application, with its hash
checked before signing and its local bundle signature verified. No test project was added.

The author's subsequent screenshot clarified that the vertical nesting guides must also be
removed. Nested rows now use indentation alone. The four workspace gates passed again, and the
rebuilt existing macOS QA Editor visibly showed Track, both Keys and the event without left-side
guide lines. The QA source remained byte-for-byte identical to its original.

## Block spacing and controls correction (2026-09-29)

```text
Requested layout correction
├── Content is left aligned; large left padding replaced by the line-number gutter
├── Row numbers sit 24 px before their cards instead of 50 px
├── Hover insertion arrows and their outside right gutter removed
├── Running rebuilt macOS Editor: compact left spacing and no arrows on a hovered selected Key
└── Original QA source preserved; no added test project
```

The current source passed formatting, workspace check, Clippy with warnings denied and all
workspace tests. The current release Editor was installed in the existing QA bundle and its
local signature verified before the visual check.

The author requested tighter spacing once more. Row numbers are now right aligned with a 4 px
gap to the card; the gutter starts at 16 px and grows only for additional digits. The extra 8 px
base row indentation was removed, including the Scene-end draft. The rebuilt macOS QA Editor
visibly showed the tighter layout. All four workspace gates passed again; QA source was preserved.

## Text authoring assistance (2026-09-29)

```text
Text (.shou)
├── File tabs and Text/Blocks switch share one row; switch stays outside the scrolling tabs
├── Faint inline suggestions by default; Tab or plain Right accepts the visible suffix
│   ├── Existing Block insertion templates and canonical dotted command names
│   ├── Shared Inspector named arguments, enums and project resource/Scene options
│   └── No rewriting narration/comments, selections or an existing token suffix
├── Enter preserves leading indentation; structural pairs add one 2-space level
├── Double quotes, parentheses, brackets and braces pair, skip their closer and undo normally
│   └── Cached lexical context disables pairing within strings/comments
└── Syntax diagnostics use the current native parser, with subtle error-position marks and hover details
    ├── 180 ms cancellable debounce, background parsing, source equality before applying
    └── Project-wide semantic/resource validation remains in Problems/Preview
Blocks
├── Plain Delete/Backspace in an empty inline Text editor deletes its owning Block
├── Empty unsaved drafts disappear; source-backed speech and associated Text Ending delete together
└── IME composition/nonempty text/modifier editing retains normal input handling; source deletion is undoable
Inspector
└── Tighter group gaps and label/input spacing
```

The pinned gpui-kit/base 0.6.4 source is the primary API evidence: `CompletionProvider` offsets
and `InlineCompletionItem.insert_text` are a cursor byte offset and an insertion suffix;
`EditorMode::accept_inline_completion` uses the existing undo/Change edit path. Tab already invokes
that path, and Right captures the native MoveRight action before the input handler, invoking the same public
mode trait only for the focused source editor. Empty Text deletion captures native Backspace/Delete
actions as well, because bound actions run before raw key callbacks.
`LanguageConfig` owns structural pairing/indentation. Direct `anyhow`/`lsp-types` dependencies expose
these existing transitive provider interface types; no service or additional completion backend is used.
Runtime acceptance and current gate results are recorded after verification below.


Current acceptance: formatting, workspace check, Clippy and all workspace tests passed;
`cargo validate tests/fixtures/native-smoke` passed with one Scene/Action and no warnings.
The rebuilt release Editor was installed in the existing macOS QA bundle; copied binary and
local signature were verified. Computer Use against that app confirmed:

```text
macOS Editor
├── Document tabs and Text/Blocks switch on one row, switching in both directions
├── Faint dotted-command suffix accepted by Right; named duration argument accepted by Tab
├── Enter retains two-space indentation; double quotes pair and skip their existing closer
├── Syntax error position receives a subtle red background; hover shows parser message
│   └── Repair clears both the mark and the stale hover detail without moving the mouse
├── Focused empty Text Blocks delete with Backspace/Delete; Cmd-Z restores deletion
│   ├── Source text confirms only the owning empty Block was removed
│   ├── Nonempty Text keeps ordinary character Backspace
│   └── Empty uncommitted draft disappears on Backspace
└── Inspector group spacing is visibly tighter; original QA source restored byte-for-byte
```

Native diagnostic hover worked, while its stock wavy underline was not visibly distinguishable
in this macOS build. The Editor therefore adds a faint error-position background through the
existing decoration collection API; diagnostics remain the sole owner of ranges/messages.
No test project was added. P7 Engine/package and deferred Windows/Linux acceptance remain separate.
