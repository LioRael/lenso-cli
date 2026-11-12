import { useState, type ComponentProps, type ComponentType } from "react"

type Button = ComponentType<ComponentProps<"button"> & { variant?: "default" | "outline" | "secondary" }>
const modules = import.meta.glob<{ Button: Button }>("./components/ui/button.tsx", { eager: true })
const InstalledButton = modules["./components/ui/button.tsx"]?.Button

export default function App() {
  const [clicks, setClicks] = useState(0)
  return <main>
    <p className="eyebrow">Lenso CLI · M1</p>
    <h1>From Registry to React.</h1>
    <p className="description">The button below is loaded from the component installed by the shadcn Wasm preset.</p>
    {InstalledButton ? <div className="demo">
      <InstalledButton onClick={() => setClicks(count => count + 1)}>Clicked {clicks} times</InstalledButton>
      <InstalledButton variant="outline">Outline</InstalledButton>
      <InstalledButton variant="secondary">Secondary</InstalledButton>
    </div> : <pre>lenso add button</pre>}
  </main>
}
