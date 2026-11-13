import { pathToFileURL } from "node:url";
import { existsSync } from "node:fs";
const [file, provider] = process.argv.slice(1);
const module = await import(pathToFileURL(file));
let stop;
if (provider === "pi") {
  const callbacks = new Map();
  module.default({ on(name, fn) { callbacks.set(name, fn); } });
  callbacks.get("session_start")({}, { hasUI: true, sessionManager: { getSessionId() { return "ses_route"; } } });
  stop = () => callbacks.get("session_shutdown")();
} else {
  const hooks = await module.CovenAgentReferencePlugin();
  await hooks.event({ event: { type: "session.created", properties: { sessionID: "ses_route", info: {
    id: "ses_route", slug: "root", projectID: "project", directory: "/private", title: "private", version: "1.19.5", time: { created: 0, updated: 1 },
  } } } });
  stop = () => hooks.dispose();
}
try {
  for (let i = 0; i < 150 && !existsSync(process.env.COVEN_ROUTE_LOG); i++) {
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  if (!existsSync(process.env.COVEN_ROUTE_LOG)) throw Error("reporter did not execute");
} finally {
  await stop();
}
