import { StrictMode, useState } from "react"
import { createRoot } from "react-dom/client"
import "./index.css"

function App() {
  const [count, setCount] = useState(0)
  return <main><h1>Your project</h1><button onClick={() => setCount(count + 1)}>Count: {count}</button></main>
}

createRoot(document.getElementById("root")!).render(<StrictMode><App /></StrictMode>)
