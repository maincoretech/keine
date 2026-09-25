# Kēne native editor test project

This is the editable Eiyashou project used to test the Editor. Open it with:

```bash
cargo editor projects/test-project
```

`scripts/main.shou` contains multiple Scenes, Text, Dialogue, If/Else, Choice, Loop, and media
blocks. `assets.yaml` and `characters.yaml` provide real manifests for Asset and Inspector tests.
Use `scratch/drag-me.md` and `scratch/destination/` to test Explorer file moves without changing the
script or manifests. Validate runtime behavior with `cargo validate projects/test-project` and
`cargo dev projects/test-project`.

The previous LetsGal 1.11.0 timeline acceptance project is preserved at
`tests/fixtures/letsgal-timeline/`. Its advanced benchmark fragment cannot be migrated
losslessly to the current Eiyashou action set.
