# Candidate only: promotion to the external tap requires retirement-gate approval.
class Coven < Formula
  desc "Magic-themed multi-agent workspaces backed by local files and kitsune"
  homepage "https://github.com/Noswad123/coven"
  url "https://github.com/Noswad123/coven.git", branch: "main"
  version "0.1.0-dev"
  license "MIT"

  depends_on "rust" => :build
  depends_on "kitsune"

  def install
    # Homebrew builds may fetch locked crates. Offline mode needs a populated
    # Cargo cache or vendored crates and is not the standard tap build path.
    system "cargo", "install", "--locked", "--path", ".", "--root", libexec
    bin.install libexec/"bin/coven"
    (pkgshare/"examples").install Dir["examples/*"]
  end

  def caveats
    <<~EOS
      coven is a native executable and does not require Python or a legacy runner.

      New workspaces default to the Herdr multiplexer. Runtime launch uses the
      workspace configuration; install Herdr or tmux and the configured agent
      commands separately as needed. Local init and task commands need no runtime.
    EOS
  end

  test do
    assert_match "Manage multi-agent coven workspaces", shell_output("#{bin}/coven --help")
    with_env(PATH: "", COVEN_HOME: nil, COVEN_EXAMPLES_DIR: nil, COVEN_COMMAND: nil) do
      workspace = testpath/"test-coven"
      system bin/"coven", "init", workspace, "--blank", "--agents", "orchestrator,architect"
      system bin/"coven", "refresh", workspace
      system bin/"coven", "task", "add", workspace, "Native install smoke"
      system bin/"coven", "task", "list", workspace, "--json"
      assert_path_exists workspace/"coven.json"
      assert_path_exists workspace/"state/tasks.json"
      assert_path_exists workspace/"tmux/coven.toml"
      system bin/"coven", "init", testpath/"demo", "--example", "interactive-demo"
    end
    assert_path_exists pkgshare/"examples/interactive-demo/example.json"
    refute_path_exists libexec/"runner"
  end
end
