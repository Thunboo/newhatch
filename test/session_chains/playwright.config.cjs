const path = require("node:path");
const os = require("node:os");
const root = path.resolve(__dirname, "../..");
const node = process.env.NEWHATCH_TEST_NODE;
module.exports = {
  testDir: "../auth",
  testMatch: ["session-chains.spec.cjs", "session-feed.spec.cjs", "frontend.spec.cjs"],
  workers: 1,
  timeout: 30000,
  reporter: "list",
  outputDir: process.env.NEWHATCH_TEST_RESULTS || path.join(os.tmpdir(), "newhatch-session-chain-results"),
  use: {
    baseURL: "http://127.0.0.1:4173", locale: "en-US",
    launchOptions: process.env.NEWHATCH_TEST_CHROMIUM ? { executablePath: process.env.NEWHATCH_TEST_CHROMIUM } : {},
  },
  webServer: {
    command: node ? `"${node}" frontend/node_modules/vite/bin/vite.js frontend --host 127.0.0.1 --port 4173 --strictPort` : "npm --prefix frontend run dev -- --port 4173 --strictPort",
    cwd: root, url: "http://127.0.0.1:4173", reuseExistingServer: false,
  },
};
