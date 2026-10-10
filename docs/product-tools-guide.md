# Local tool storage and recovery helpers

Studio now connects these helpers through **Saved files, backups and recovery**
on an open generated tool. The existing order-tool format and workflow remain
separate. Headless control tests use explicit synthetic dialogs; native OS
dialogs and the final packaged desktop journey require separate verification.

## Everyday file operations

- Open a saved generated-tool folder from the home page, or select a recent tool.
  Opening verifies the saved tool. Supported older formats show an explicit
  local upgrade and restart step; incompatible formats are kept unchanged.
- Open **Saved files, backups and recovery** to see the original saved output
  occurrences. **Save this exact output…** saves that occurrence without running
  its action again. The file picker suggests CSV or JSON as originally produced.
  Existing files are never replaced, even when their bytes match.
- **Save a checked backup copy…** exports a verified `.gmbak` file. **Open a
  backup file…** first checks its complete contents and presents a preview.
- Recovery suggests a fresh folder in the normal generated-tool location.
  **Choose a different parent folder…** creates a new suggestion; review its
  displayed destination before **Recover and verify this separate copy**.
  Recovery preserves the original tool and backup. Later work outside the
  backup is not included, and recovery does not undo external actions.
- A recovered copy is reopened for verification and gets its own recent entry
  and automatic checkpoint shelf. The original stays selected until you choose
  **Open the verified separate copy**.
- **Check saved work** diagnoses the current tool. The home page also offers
  **Check this tool or recover a backup** for each exact recent instance, even
  when its current pointer or snapshot is damaged. Only checked automatic
  backups are offered; no backup is deleted.

Pending form or control edits must finish or be explicitly discarded before
leaving their input page. File operations run on the shared worker and honor
cancellation before their final write boundary. Once writing begins, Studio
reports the actual checked or uncertain result instead of pretending Cancel
removed an external file. A native dialog that cannot distinguish closure from
failure is reported as an indeterminate selection, not a confirmed cancellation.

If an external file attempt is interrupted, its exact selected destination is
kept in the host restart record. Restart inspects it but never repeats the write.
Matching output bytes or a valid tool at that location are inspection evidence,
not proof that the interrupted operation succeeded. Keep any files there and
finish reviewing the attempt before starting another file write. Ordinary saved
work remains available.

If an unfinished original save cannot be reconciled because its files are
damaged, you can still recover a verified backup into a separate folder. Only
**Keep the original attempt and open this verified separate copy** moves that
exact unconfirmed attempt into preserved host history and enables new work in
the verified copy. The original attempt, including entered values or its full
change plan, remains available for read-only review after restart. It is never
retried or described as committed. The saved copy and the pending association
are checked again before that explicit switch. Existing entries are never
dropped: a full history (16 entries) or the unchanged 1 MiB host-journal limit
refuses the switch while preserving the original attempt and recovered files.
Inspection and returning home remain available.

No credentials, additional permissions or new project storage schema are
introduced. These are additive optional host-journal records; old records
without them keep their existing format and read path.

## What is preserved

A backup contains one committed project snapshot and exactly the immutable
intention objects reachable from its decision and adoption history. Records,
stable identifiers, later field values, business events, output artifacts,
programs, operation receipts, session and decision history are retained.
The capture reads the store's committed pointer and immutable objects. It does
not copy an arbitrary directory, include unfinished writes, or treat an orphan
snapshot as a committed checkpoint.

Recovery creates a separate, fresh tool. It never replaces current work, deletes
the original, or removes backups. Restoring a mistaken software behavior is a
different operation: the intention controller must check and adopt compatible
behavior on **current data**, retaining later work. An older backup is not an
implementation of that operation.

Backups are local and usable without a provider, login, server or network call.
They contain full work data and accepted example scenes, so they should be kept
private. Integrity checks detect damage and invalid references; they do not
authenticate a file's author or turn imported historical check flags into fresh
adoption authority.

Local checkpoints can recover interrupted operations or damaged project files.
They do not protect against losing the drive that holds both the tool and its
checkpoints. Exporting a verified copy to another deliberately selected location
provides a separate copy; the helpers never upload it to a cloud service.

## Locations and recent tools

`ToolLocations::default_location()` creates the managed directory beneath:

- Linux: `XDG_DATA_HOME`, or the profile's `.local/share`
- macOS: the profile's `Library/Application Support`
- Windows: `LOCALAPPDATA`, or the profile's `AppData/Local`

The managed suffix is `GitManager/generated-tools`. Newly created Unix
directories are private. Existing permissions are not broadened or rewritten.
An invalid configured location fails visibly instead of silently choosing a
different place. `ToolLocations::chosen()` accepts an existing deliberately
selected folder; an unavailable choice remains unavailable until the person
reconnects it or selects another folder.

`new_tool_path()` supplies a fresh child-name suggestion. It is not a
reservation: `ProductStore::create` or verified recovery must still perform
atomic fresh-only activation. Ordinary users should use a name and a folder
dialog, never type absolute paths or edit saved files.

`RecentTools` stores only bounded navigation metadata in `recent-tools.json`,
separate from project data. `remember()` returns an instance ID. `relocate()`
requires a deliberate folder selection matching the saved project and initial
program identity; it preserves the instance ID. Recovery uses the composed
`recover_tool()` operation, which checks the recent list before creating a target
and allocates a fresh instance ID. A listed but missing path is refused until the
person selects a different destination or locates the original. Exact recovery
still retains the original project/record IDs. `remember()` is for opening an
existing tool, not for registering a separately restored copy at an old path.
Existing-entry matching and relocation also compare opened directory identities;
case/Unicode aliases cannot merge two registered instances or their checkpoints.

Recent status is `Located`, `Missing`, `Replaced` or `Unverified`. `Located`
means the folder and basic store identity were found; it is **not** a complete
intention-package verification. Opening must use `open_verified()` again.
Corrupt/future recent metadata is preserved and reported, not reset to an empty
list. Metadata is bounded to 512 entries and 1 MiB; entries are not silently
evicted to hide a limit.

## Verified backup APIs

- `VerifiedBackup::capture(&ProductStore)` loads one committed snapshot and
  asks `IntentArchive::export_for` for its exact reachable object set
- `from_bytes()` and `read()` validate the format, canonical representation,
  checksum, snapshot/runtime and complete typed intention bundle
- `export_new(path)` writes a new file atomically, then verifies its readback;
  an existing destination is refused even when its bytes match
- `recover_tool(path, recent, opened_unix_ms)` validates before creating the
  destination, checks and locks the recent list, stages and rechecks the bundle
  through the trusted pre-CURRENT store callback, verifies the exact activated
  snapshot, then records a fresh instance. Before activation it also compares
  opened directory identities, refusing case/Unicode aliases of listed paths.
  A refusal can leave an unactivated new folder with no CURRENT; it preserves
  the original and never advertises that folder as a usable recovered tool.
  It returns `CreatedTool { store,
  registration }`: a registration warning after creation preserves the usable
  store and must not be reported as lost recovered data or retried as creation
- `open_verified(path, expected_identity)` checks the actual store, retained
  outputs and intention objects; recent-tool opening supplies its saved identity

The single-file `.gmbak` format is bounded canonical JSON, not a ZIP/TAR archive.
No imported path is extracted. The outer limit is 64 MiB; the snapshot limit is
16 MiB. The intention archive enforces at most 512 objects, 1 MiB per object
and 32 MiB for the encoded bundle and its aggregate object bytes. Unknown
formats, extra/duplicate/noncanonical content, missing or wrong objects,
checksum mismatch, unsupported runtime state, links and destination collisions
fail closed. Interrupted temporary files are not usable backups.

## Automatic checkpoint integration

`CheckpointShelf::for_tool(locations, identity, instance_id)` creates the
checkpoint namespace. Use the stable recent-entry instance ID so a recovered
copy cannot replace the original instance's checkpoint sequence. A deliberate
relocation keeps the same instance ID and checkpoint namespace.

The controller uses these boundaries:

1. Open through `open_verified`; retain the returned snapshot/revision and
   actual runtime outputs for the normal optimistic operation checks
2. Remember the verified location and initialize its checkpoint shelf
3. Capture automatically after open and every successful committed save or
   adoption, and before a potentially risky behavior/recovery operation
4. Treat failure after a committed save as a separate backup warning; never
   undo that save or report it as uncommitted. Block a risky transition when
   its required pre-operation checkpoint cannot be verified
5. Present backup export and recovery through native file/folder selection,
   sensible suggested names, counts and an explicit fresh-target confirmation
6. Recover through `recover_tool`, then use its verified store and successful
   registration ID for the new checkpoint shelf. If registration failed after
   creation, keep the tool open and show the explicit navigation warning; do
   not allocate a shelf from the original entry or repeat fresh creation.
   Keep the original entry and all original files

`capture()` is idempotent for identical content and verifies actual stored
bytes, including on retry. Checkpoints use immutable content-addressed names;
there is no mutable index that could advertise an interrupted write. No
retention purge or automatic deletion is performed. Disk/full/unavailable
errors are surfaced with a safe next step.

`newest_verified()` bounds discovery to 4096 directory entries and checks at
most the 64 highest named revisions, skipping corrupt candidates only with
explicit diagnostics. It offers a checkpoint only after full validation of its
content, filename and tool identity. If older files remain unchecked, that
limit is reported; the person can choose one backup file for verification.
“Newest” means highest verified store revision within this instance, not an
unverified filesystem timestamp or a claim that every historical file was read.

## Diagnosis and failure behavior

`doctor()` is read-only. A damaged CURRENT, snapshot or intention object is not
silently repaired. The report separates a verified current tool, a plain-language
problem and next step, technical detail, and any separately verified recovery
candidate. A missing checkpoint never becomes a fabricated success. The
controller must let the person inspect/choose the recovery copy rather than
automatically replace the current one.

Path operations reuse the store's pinned descriptor/no-reparse implementation.
Traversal, symlink/hard-link redirection, replaced folders and collisions are
refused. These helpers do not grant generated programs file access, change
authentication or security configuration, or publish a release. Older generated
tools and backups use only the supported checked upgrade paths.

Component fixtures, headless host controls and CI are implementation evidence.
Ordinary-person operation, native dialogs/IME and integrated current-build
packaged product acceptance remain separate verification work.
