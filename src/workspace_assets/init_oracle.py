"""Test-only Python byte oracle for native initial workspace assets."""
import json
from runner import coven

agents = ["orchestrator", "architect", "implementer", "reviewer", "clippy",
          "srikanth", "jamal", "kris", "eric", "CLIPPY", "CUSTOM",
          "café 😀", 'quote"\\\n\r\t\b\f\x00\x1f\x7f']
teams = []
for roster in [[], agents, ["clippy"], ["CUSTOM"]]:
    for name in ["", "A coven", 'A \'name\' " \\ café 😀\x7f\n\r\t\b\f\x00\x1f']:
        for multiplexer in ["herdr", "tmux"]:
            for display_names in [{}, {agent: "" if i % 2 else f"Display {agent} 😀"
                                       for i, agent in enumerate(roster)}]:
                command = ' oc --model "café 😀" \\ \n\t\x7f'
                teams.append(dict(name=name, agents=roster, command=command,
                                  multiplexer=multiplexer, display_names=display_names,
                                  text=coven.team_toml(name, roster, command,
                                                       multiplexer, display_names)))
example = coven.load_example("interactive-demo")
print(json.dumps(dict(
    teams=teams,
    agents={agent: coven.agent_state_text(agent) for agent in agents},
    defaults=dict(goal=coven.example_goal_text(None), work=coven.work_readme(),
                  output=coven.output_readme()),
    checkpoints={path: text for path, text in coven.workspace_structure_files().items()
                 if path.startswith("checkpoints/")},
    example=dict(id=example["id"], seed_message=example["seed_message"],
                 goal=coven.example_goal_text(example),
                 spec=coven.read_example_text(example, "spec.md"),
                 work=coven.read_example_text(example, "work/README.md"),
                 output=coven.read_example_text(example, "output/README.md"),
                 context=coven.read_example_text(example, "prompt-context.md"),
                 tasks=coven.read_example_json(example, "tasks.json", [])))))
