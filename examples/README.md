# Coven examples

`coven init` can seed a workspace from an example directory. Examples keep
the runner code small and make starter projects easy to inspect, copy, and adapt.

Default example:

```bash
coven init ~/Projects/new-coven --example interactive-demo
```

Each example is a directory with this shape:

```text
examples/<id>/
├── example.json        # name, description, seed message
├── goal.md            # copied to the generated workspace
├── spec.md            # copied to the generated workspace
├── prompt-context.md  # appended to each generated agent prompt
├── tasks.json         # seeded task ids, titles, owners, notes
├── work/README.md     # optional work directory guidance
└── output/README.md   # optional output directory guidance
```

To create your own use case, copy `interactive-demo/`, edit the Markdown and
`tasks.json`, then run:

```bash
coven init ~/Projects/my-coven --example <id>
```

Use `--blank` when you want only the workspace structure and no example seed.
