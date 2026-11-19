// Cove-owned interop fixture. Actual factories/registry/workers, no native UI or model.
import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads"
import { rename } from "node:fs/promises"

const packagePath = process.env.COVEN_RESTART_PACKAGE!
const source = (path: string) => import(`${packagePath}/${path}`)
const dependency = (name: string) => import(Bun.resolveSync(name, packagePath))

if (!isMainThread) {
  const { KitsuneAgentStatePlugin } = await source("src/plugin/kitsune-agent-state.ts")
  const { attachmentReporter } = await source("src/coven/attachment-registry.ts")
  const { createAttachmentWorker } = await source("src/coven/attachment-worker.ts")
  const { createAttachmentObserver } = await source("src/coven/attachment-observer.ts")
  const { attachmentCommand, captureAttachmentTarget } = await source("src/coven/attachment-transport.ts")
  Object.assign(process.env, workerData.env)
  let sequence = 0
  const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>()
  // The main test delegates each lookup to its actual in-process SDK. No fabricated
  // session DTO/UUID, replacement registry, or handwritten attachment transport.
  const client = { session: { get: (request: { path: { id: string } }) => new Promise((resolve, reject) => {
    const id = ++sequence
    pending.set(id, { resolve, reject })
    // AbortSignal is worker-local and is not structured-cloneable in Bun.
    parentPort!.postMessage({ id, request: { path: request.path, throwOnError: true } })
  }) } }
  parentPort!.on("message", (message) => {
    const waiter = pending.get(message.id)
    if (!waiter) return
    pending.delete(message.id)
    if (message.error) waiter.reject(new Error(message.error))
    else waiter.resolve(message.result)
  })
  const plugin = await KitsuneAgentStatePlugin({ directory: workerData.directory, client })
  const context = await captureAttachmentTarget(attachmentCommand(workerData.env), workerData.env)
  if (!context) throw new Error("actual target capture failed")
  const worker = createAttachmentWorker({ env: workerData.env })
  let sent = false
  const observer = createAttachmentObserver({ context: Promise.resolve(context), send: async (input: unknown) => {
    const revision = await worker.select(input)
    await worker.idle()
    sent = true
    return revision
  } })
  observer.select({ sessionID: workerData.session, directory: workerData.directory })
  const end = performance.now() + 5000
  while (!sent) {
    if (performance.now() > end) throw new Error("initial actual worker send timed out")
    await Bun.sleep(20)
  }
  parentPort!.postMessage({ ready: true, instance: attachmentReporter(workerData.directory)!.instance })
  parentPort!.on("message", async (message) => {
    if (message.operation !== "close") return
    await observer.close()
    await worker.close()
    await plugin.dispose?.()
    parentPort!.postMessage({ closed: true })
    parentPort!.close()
  })
} else {
  const { test } = await import("bun:test")
  const { Effect } = await dependency("effect")
  const { LayerNode } = await dependency("@opencode-ai/core/effect/layer-node")
  const { CrossSpawnSpawner } = await dependency("@opencode-ai/core/cross-spawn-spawner")
  const { SessionProjector } = await dependency("@opencode-ai/core/session/projector")
  const { testEffectShared } = await source("test/lib/effect.ts")
  const { InstanceRef } = await source("src/effect/instance-ref.ts")
  const { SessionStatus } = await source("src/session/status.ts")
  const { Session } = await source("src/session/session.ts")
  const { EventV2Bridge } = await source("src/event-v2-bridge.ts")
  const root = process.env.COVEN_RESTART_INTEROP
  if (!root) test.skip("same-SID reporter restart (opt-in)", () => {})
  else testEffectShared(
    LayerNode.compile(LayerNode.group([Session.node, SessionStatus.node, SessionProjector.node, EventV2Bridge.node, CrossSpawnSpawner.node])),
  ).instance("same native SID, isolated actual reporter factories and live second root", () => Effect.gen(function* () {
    const { createOpencodeClient } = yield* Effect.promise(() => dependency("@opencode-ai/sdk"))
    const { Server } = yield* Effect.promise(() => source("src/server/server.ts"))
    const ctx = yield* InstanceRef
    if (!ctx) return yield* Effect.die("missing instance")
    const client = createOpencodeClient({ baseUrl: "http://test", directory: ctx.directory,
      fetch: (request: Request) => Server.Default().app.fetch(request) })
    const a = (yield* Effect.promise(() => client.session.create({ body: { title: "Restart root" }, throwOnError: true }))).data
    const c = (yield* Effect.promise(() => client.session.create({ body: { title: "Other live root" }, throwOnError: true }))).data
    const env = yield* Effect.promise(() => Bun.file(`${root}/pane-env.json`).json())
    const otherEnv = yield* Effect.promise(() => Bun.file(`${root}/c/pane-env.json`).json())
    const children = new Map<string, { worker: Worker; instance?: string; closed: boolean; error?: string }>()
    const wait = async (predicate: () => unknown) => {
      const end = performance.now() + 10000
      while (!predicate()) {
        if (performance.now() > end) throw new Error("reporter worker timed out")
        await Bun.sleep(20)
      }
    }
    const start = async (name: string) => {
      const child = { worker: new Worker(import.meta.filename, { workerData: {
        env: name === "c" ? otherEnv : env, directory: ctx.directory, session: name === "c" ? c.id : a.id,
      } }), instance: undefined as string | undefined, closed: false, error: undefined as string | undefined }
      children.set(name, child)
      child.worker.on("error", (error) => { child.error = String(error) })
      child.worker.on("message", async (message) => {
        if (message.ready) child.instance = message.instance
        else if (message.closed) child.closed = true
        else if (message.request) {
          try {
            const result = await client.session.get(message.request)
            child.worker.postMessage({ id: message.id, result: { data: result.data } })
          } catch (error) { child.worker.postMessage({ id: message.id, error: String(error) }) }
        }
      })
      await wait(() => child.instance || child.error)
      if (child.error) throw new Error(child.error)
      return { instance: child.instance, session: name === "c" ? c.id : a.id }
    }
    yield* Effect.addFinalizer(() => Effect.promise(async () => {
      await Promise.all([...children.values()].map((child) => child.worker.terminate()))
      await client.instance.dispose({ throwOnError: true })
    }))
    yield* Effect.promise(async () => {
      for (let n = 1; n <= 8; n++) {
        const file = `${root}/restart-command-${n}.json`
        const end = performance.now() + 20000
        while (!(await Bun.file(file).exists())) {
          if (performance.now() > end) throw new Error("restart controller timed out")
          await Bun.sleep(20)
        }
        const { operation } = await Bun.file(file).json()
        let result = {}
        if (operation.startsWith("start-")) result = await start(operation.slice(6))
        else if (operation === "stop-a") {
          // Abrupt frontend loss: no observer/worker/plugin disposal or detach.
          await children.get("a")!.worker.terminate()
        } else if (operation === "stop-c") {
          const child = children.get("c")!
          child.worker.postMessage({ operation: "close" })
          await wait(() => child.closed || child.error)
          if (child.error) throw new Error(child.error)
        } else if (operation !== "done") throw new Error("unknown restart operation")
        const reply = `${root}/restart-reply-${n}.json`
        await Bun.write(reply + ".new", JSON.stringify(result))
        await rename(reply + ".new", reply)
        if (operation === "done") return
      }
      throw new Error("restart command bound exceeded")
    })
  }), { config: { formatter: false, lsp: false, plugin: [] } }, { timeout: 60000 })
}
