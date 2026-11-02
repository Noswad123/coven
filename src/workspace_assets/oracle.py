"""Test-only byte oracle for frozen native refresh asset profile v1."""
import json
import shlex
from pathlib import Path
from runner import coven

cases = []
agents = ["orchestrator", "architect", "implementer", "reviewer", "clippy",
          "srikanth", "jamal", "kris", "eric", "CLIPPY", "CUSTOM"]
commands = ["oc", " oc --model 'a b' ", "oc\t--x", "oc\n--x",
            "oc  ", "opencode --flag", "\x1coc\x1f", "oc $VAR; printf 'x'"]
roots = ["/tmp/blank", "/tmp/café 😀/a\"\x7f"]
for index, agent in enumerate(agents):
    for example in [False, True]:
        root = Path(roots[index % 2])
        command = commands[index % len(commands)]
        cli = "coven" if index % 2 else "/tmp/a'b café/coven"
        coven.cli_command = lambda: shlex.quote(cli)
        context = coven.read_example_text(coven.load_example("interactive-demo"), "prompt-context.md") if example else ""
        cases.append(dict(root=str(root), agent=agent, command=command,
                          cli=cli, example=example, role=coven.agent_role(agent),
                          prompt=coven.prompt_text(agent, root, context),
                          starter=coven.starter_prompt_text(agent, root),
                          normalized=coven.normalize_opencode_command(command),
                          shell_quote=shlex.quote(cli),
                          manifest=coven.tmux_manifest("A 'name' \" \x7f café 😀", root, [agent], command)))
print(json.dumps(cases))
