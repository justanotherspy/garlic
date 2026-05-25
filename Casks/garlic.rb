cask "garlic" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.3.1"
  sha256 arm:   "660dced4439d5fb4de452257e8d3980f2a76dc599813d83b6b146ff91fb73f62",
         intel: "1f65566630367e674793298c37809c33bd755609ccd34b03d655c2fdc71d47ec"

  url "https://github.com/justanotherspy/garlic/releases/download/v#{version}/garlic-#{arch}-apple-darwin.tar.gz",
      verified: "github.com/justanotherspy/garlic/"
  name "Garlic"
  desc "Track active coding time with Claude Code and nudge yourself to take breaks"
  homepage "https://github.com/justanotherspy/garlic"

  livecheck do
    url :url
    strategy :github_latest
  end

  binary "garlic"

  caveats <<~EOS
    Garlic hooks into Claude Code. To install the hooks, run:
      garlic setup
  EOS
end
