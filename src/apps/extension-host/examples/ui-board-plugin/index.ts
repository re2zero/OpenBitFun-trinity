// R7c smoke fixture: a real extension-host plugin that contributes the same
// board scene as the R7b in-process fixture and serves it through the
// `host.ui.*` channel. State lives in the plugin closure; every settled event
// returns the replacement tree document (design §7.3).
const state = { count: 0, name: "" }

function boardDocument() {
  return {
    schemaVersion: 1,
    root: {
      type: "column",
      props: { gap: 3, padding: 4 },
      children: [
        { type: "heading", props: { text: "@title", level: 1 } },
        { type: "text", props: { text: `count = ${state.count}`, tone: "muted" } },
        { type: "text", props: { text: `name = ${state.name}` } },
        {
          type: "input",
          props: { placeholder: "@name.hint", value: state.name },
          on: { submit: "set-name" },
        },
        {
          type: "button",
          props: { label: "@btn.increment", tone: "primary" },
          on: { press: "increment" },
        },
      ],
    },
  }
}

const UiBoardPlugin = async () => {
  return {
    ui: {
      event: async (input: { sceneID: string; action: string; payload?: { kind: string; value: unknown } }) => {
        if (input.sceneID !== "board") return undefined
        if (input.action === "increment") {
          state.count += 1
          return boardDocument()
        }
        if (input.action === "set-name") {
          const payload = input.payload
          if (payload && payload.kind === "text") {
            state.name = String(payload.value)
            return boardDocument()
          }
          return undefined
        }
        return undefined
      },
    },
  }
}

export default {
  id: "ui-board",
  server: UiBoardPlugin,
}
