class Aimemo < Formula
  desc "Persistent memory for AI coding agents"
  homepage "https://github.com/rustkit-ai/aimemo"
  version "0.1.9"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/rustkit-ai/aimemo/releases/download/v#{version}/aimemo-aarch64-apple-darwin.tar.gz"
      sha256 :no_check
    end
    on_intel do
      url "https://github.com/rustkit-ai/aimemo/releases/download/v#{version}/aimemo-x86_64-apple-darwin.tar.gz"
      sha256 :no_check
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/rustkit-ai/aimemo/releases/download/v#{version}/aimemo-aarch64-unknown-linux-gnu.tar.gz"
      sha256 :no_check
    end
    on_intel do
      url "https://github.com/rustkit-ai/aimemo/releases/download/v#{version}/aimemo-x86_64-unknown-linux-gnu.tar.gz"
      sha256 :no_check
    end
  end

  def install
    bin.install "aimemo"
  end

  test do
    assert_match "aimemo #{version}", shell_output("#{bin}/aimemo --version")
  end
end
