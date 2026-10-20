import { useEffect, useRef, useState } from "react";

/** A quiet, explicitly played local sound; no file, microphone or provider. */
export default function GenesisChime() {
  const audio = useRef<AudioContext | null>(null);
  const [playing, setPlaying] = useState(false);
  const [notice, setNotice] = useState("");
  useEffect(() => () => { void audio.current?.close().catch(() => undefined); }, []);
  const play = async () => {
    if (audio.current) { await audio.current.close(); audio.current = null; setPlaying(false); return; }
    if (!("AudioContext" in window)) { setNotice("Sound is unavailable in this browser."); return; }
    const context = new AudioContext();
    audio.current = context;
    try {
      await context.resume();
      setNotice(""); setPlaying(true);
      [261.63, 392, 523.25].forEach((frequency, index) => {
        const voice = context.createOscillator();
        const gain = context.createGain();
        const start = context.currentTime + index * .18;
        voice.type = "sine"; voice.frequency.value = frequency;
        gain.gain.setValueAtTime(0, start);
        gain.gain.linearRampToValueAtTime(.035, start + .08);
        gain.gain.exponentialRampToValueAtTime(.0001, start + 1.5);
        voice.connect(gain); gain.connect(context.destination);
        voice.start(start); voice.stop(start + 1.6);
        if (index === 2) voice.onended = () => {
          if (audio.current === context) { audio.current = null; setPlaying(false); }
          void context.close().catch(() => undefined);
        };
      });
    } catch {
      audio.current = null; setPlaying(false); setNotice("Sound could not start. Your Entity is ready.");
      void context.close().catch(() => undefined);
    }
  };
  return <div><button type="button" onClick={() => void play()}>{playing ? "Stop chime" : "Play awakening chime"}</button>{notice && <p role="status">{notice}</p>}</div>;
}
