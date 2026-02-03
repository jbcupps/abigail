import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import BootSequence from "./components/BootSequence";
import ChatInterface from "./components/ChatInterface";

function App() {
  const [birthComplete, setBirthComplete] = useState<boolean | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const complete = await invoke<boolean>("get_birth_complete");
        setBirthComplete(complete);
      } catch {
        setBirthComplete(false);
      }
    })();
  }, []);

  const onBirthComplete = () => setBirthComplete(true);

  if (birthComplete === null) {
    return (
      <div className="min-h-screen bg-black text-green-500 font-mono flex items-center justify-center">
        Loading...
      </div>
    );
  }

  if (!birthComplete) {
    return <BootSequence onComplete={onBirthComplete} />;
  }

  return <ChatInterface />;
}

export default App;
