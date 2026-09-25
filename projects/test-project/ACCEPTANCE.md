# Native Editor acceptance

1. Open `scripts/main.shou`. Text and Blocks must show the same source. Drag the two adjacent
   narration blocks past one another, then use Undo (⌘Z) and Redo (⇧⌘Z).
2. In Blocks, edit Aya's dialogue, create an empty Text block with Enter, and insert an in-block
   line break with Shift+Enter. Inspect Choice, If/Else, Loop, and Scene controls.
3. In Explorer, move `scratch/drag-me.md` into `scratch/destination/`. The target must highlight and
   receive the file. With Explorer focused, ⌘Z restores the original path and ⇧⌘Z moves it back.
   Repeat for a new scratch file, rename, copy, and delete; an open file must be closed before a
   path-changing operation.
4. In Assets, filter by type, folder, and tags. Select each background, figure, and sound; the
   Inspector must show the corresponding manifest entry. Change a tag or ID in Inspector, then use
   ⌘Z and ⇧⌘Z to verify the manifest and any script references change together.
5. In Characters, inspect Aya and add a temporary second character, then inspect `characters.yaml`.
6. Run `cargo validate projects/test-project`; the project must have no errors.
