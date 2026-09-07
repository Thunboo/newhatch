module.exports = {
  testDir: ".",
  testMatch: "frontend.spec.cjs",
  workers: 1,
  reporter: "list",
  use: { baseURL: "http://127.0.0.1:4173", launchOptions: { executablePath: "/usr/bin/chromium-browser", args: ["--no-sandbox"] } },
  outputDir: "/results",
  webServer: {
    command: "npm --prefix frontend run dev -- --port 4173 --strictPort",
    url: "http://127.0.0.1:4173",
  },
};
