cask "safe-your-eyes" do
  arch arm: "arm64", intel: "x86_64"

  version "0.5.0"
  sha256 arm:   "b4139ae41f0455f08de55d0a1e3ac123131f72579202b0f9c4c6ac110cfde7bb",
         intel: "ff863139c83b309730e4b1b1c7f19d4bffcdafae1aa2a7802611131e5c47a981"

  url "https://github.com/tunglt1810/safe-your-eyes/releases/download/v#{version}/safe-your-eyes-macos-#{arch}.zip"
  name "Safe Your Eyes"
  desc "Lightweight cross-platform Rust eye-protection desktop app"
  homepage "https://tunglt1810.github.io/safe-your-eyes/"

  app "Safe Your Eyes.app"
  binary "#{appdir}/Safe Your Eyes.app/Contents/MacOS/safe-your-eyes"

  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/Safe Your Eyes.app"]
  end

  uninstall quit: "io.github.safeyoureyes.SafeYourEyes"

  zap trash: [
    "~/Library/Application Support/safe-your-eyes",
    "~/Library/Caches/safe-your-eyes",
    "~/Library/LaunchAgents/io.github.safeyoureyes.SafeYourEyes.plist",
  ]
end
