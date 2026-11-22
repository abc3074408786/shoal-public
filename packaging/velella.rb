# Homebrew formula — build from source until we publish bottles.
# Install via a tap:
#   brew tap velella-dev/velella https://github.com/velella-dev/homebrew-velella
#   brew install velella
class Velella < Formula
  desc "A colony of agents, sailing while you sleep"
  homepage "https://github.com/velella-dev/velella"
  url "https://github.com/velella-dev/velella/archive/refs/tags/v0.1.0.tar.gz"
  license "Apache-2.0"

  depends_on "rust" => :build
  # agents are external CLIs (claude, codex, …) — velella drives whatever
  # the user already has

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "crates/velella"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/velella version")
  end
end
