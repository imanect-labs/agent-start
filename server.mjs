import { createServer } from "node:http";
import next from "next";
import { attachTerminalWs } from "./server/terminal.mjs";

const dev = process.env.NODE_ENV !== "production";
const port = Number(process.env.PORT ?? 3030);
const host = process.env.HOST ?? "0.0.0.0";

const app = next({ dev, hostname: host, port });

await app.prepare();

const handle = app.getRequestHandler();
const nextUpgradeHandler =
  typeof app.getUpgradeHandler === "function" ? app.getUpgradeHandler() : null;

const server = createServer((req, res) => handle(req, res));

const terminalUpgrade = attachTerminalWs(server);

server.on("upgrade", async (req, socket, head) => {
  const handled = await terminalUpgrade(req, socket, head);
  if (handled) return;
  if (nextUpgradeHandler) {
    nextUpgradeHandler(req, socket, head);
  } else {
    socket.destroy();
  }
});

server.listen(port, host, () => {
  console.log(`ccstart server: http://${host}:${port} (dev=${dev})`);
});
