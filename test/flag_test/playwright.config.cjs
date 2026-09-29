const path = require("node:path");
const root = path.resolve(__dirname, "../..");

module.exports = {
  testDir: "..",
  testMatch: ["**/auth/*.spec.cjs", "**/flag_test/replay.spec.cjs"],
  workers: 1,
  timeout: 30_000,
  reporter: "list",
  outputDir: "/results",
  use: {
    baseURL: "http://127.0.0.1:4173",
    locale: "en-US",
    launchOptions: { executablePath: "/usr/bin/chromium-browser", args: ["--no-sandbox"] },
  },
  webServer: [
    {
      command: "node test/flag_test/replay-echo.cjs",
      cwd: root,
      url: "http://127.0.0.1:18082/health",
    },
    {
      command: 'nginx -p "' + root + '/" -c test/flag_test/replay-nginx.conf -g "daemon off;"',
      cwd: root,
      url: "http://127.0.0.1:18080/flag",
    },
    {
      command: "npm --prefix frontend run dev -- --port 4173 --strictPort",
      cwd: root,
      url: "http://127.0.0.1:4173",
    },
  ],
};
