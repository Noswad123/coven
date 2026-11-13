// Development-only native event input. No SDK, provider, model, or UI startup.
import { createInterface } from "node:readline"
import { pathToFileURL } from "node:url"

const { KitsuneAgentStatePlugin } = await import(pathToFileURL(process.argv[2]!).href)
const plugin = await KitsuneAgentStatePlugin({})
console.log(JSON.stringify({ ready: true }))
try {
  for await (const line of createInterface({ input: process.stdin })) {
    await plugin.event({ event: JSON.parse(line) })
    // The real reporter intentionally queues asynchronously. This is a pacing
    // barrier, not a fabricated delivery acknowledgement; views prove delivery.
    await Bun.sleep(150)
    console.log(JSON.stringify({ dispatched: true }))
  }
} finally {
  await plugin.dispose?.()
}
