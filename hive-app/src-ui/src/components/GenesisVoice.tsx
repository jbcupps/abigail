import { useEffect, useRef, useState } from "react";
import { detectRuntimeMode } from "../runtimeMode";

async function nativeSpeech<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

interface RecognitionResult { isFinal: boolean; 0: { transcript: string } }
interface RecognitionEvent { resultIndex: number; results: ArrayLike<RecognitionResult> }
interface Recognition {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult: ((event: RecognitionEvent) => void) | null;
  onerror: ((event: { error: string }) => void) | null;
  onend: (() => void) | null;
  start: () => void;
  stop: () => void;
  abort: () => void;
}
type SpeechWindow = Window & { SpeechRecognition?: new () => Recognition; webkitSpeechRecognition?: new () => Recognition };

export default function GenesisVoice({ onTranscript, onCommand, onSubmit, response, disabled }: {
  onTranscript: (text: string) => void;
  onCommand: (command: string) => void;
  onSubmit: (text: string) => void;
  response: string;
  disabled: boolean;
}) {
  const [consent, setConsent] = useState(false);
  const [listening, setListening] = useState(false);
  const [handsFree, setHandsFree] = useState(false);
  const [notice, setNotice] = useState("");
  const [interim, setInterim] = useState("");
  const recognition = useRef<Recognition | null>(null);
  const attempt = useRef(0);
  const native = detectRuntimeMode() === "native";
  const speechWindow = window as SpeechWindow;
  const RecognitionApi = speechWindow.SpeechRecognition ?? speechWindow.webkitSpeechRecognition;
  const canSpeak = "speechSynthesis" in window && "SpeechSynthesisUtterance" in window;

  useEffect(() => {
    if (!consent || !handsFree || !canSpeak || !response) return;
    window.speechSynthesis.cancel();
    window.speechSynthesis.speak(new SpeechSynthesisUtterance(response));
    return () => window.speechSynthesis.cancel();
  }, [consent, handsFree, canSpeak, response]);

  useEffect(() => () => {
    attempt.current += 1;
    if (recognition.current) {
      recognition.current.onresult = null;
      recognition.current.onerror = null;
      recognition.current.onend = null;
      recognition.current.abort();
    }
    if ("speechSynthesis" in window) window.speechSynthesis.cancel();
    if (native) void nativeSpeech("cancel_genesis_speech").catch(() => undefined);
  }, [native]);

  const acceptTranscript = (text: string) => {
    const clean = text.trim();
    if (!clean) return;
    const command = clean.toLowerCase().replace(/[.!?]+$/, "");
    if (["accept suggestions", "review entity", "awaken entity", "skip question", "save and exit"].includes(command)) onCommand(command);
    else if (handsFree) onSubmit(clean);
    else onTranscript(clean);
  };

  const stop = () => {
    attempt.current += 1;
    recognition.current?.abort();
    if (native) void nativeSpeech("cancel_genesis_speech").catch(() => undefined);
    setListening(false); setInterim("");
  };

  const listen = async () => {
    if (listening) { stop(); return; }
    if ((!RecognitionApi && !native) || !consent || disabled) return;
    setNotice(""); setInterim("");
    const currentAttempt = ++attempt.current;
    if (!RecognitionApi && native) {
      setListening(true);
      try {
        const text = await nativeSpeech<string>("transcribe_genesis_speech", { consent: true });
        if (currentAttempt === attempt.current) acceptTranscript(text);
      } catch (error) {
        if (currentAttempt === attempt.current) setNotice(typeof error === "string" ? error : "Windows dictation could not start. You can continue by typing.");
      } finally { if (currentAttempt === attempt.current) setListening(false); }
      return;
    }
    if (!RecognitionApi) return;
    const next = new RecognitionApi();
    next.continuous = false;
    next.interimResults = true;
    next.lang = navigator.language || "en-US";
    next.onresult = (event) => {
      let transcript = "";
      let partial = "";
      for (let i = event.resultIndex; i < event.results.length; i += 1) {
        if (event.results[i].isFinal) transcript += `${event.results[i][0].transcript} `;
        else partial += `${event.results[i][0].transcript} `;
      }
      if (currentAttempt === attempt.current) {
        setInterim(partial);
        acceptTranscript(transcript);
      }
    };
    next.onerror = (event) => { setNotice(`Voice input stopped (${event.error}). You can continue by typing.`); setListening(false); };
    next.onend = () => { if (currentAttempt === attempt.current) { setListening(false); setInterim(""); } };
    recognition.current = next;
    try { next.start(); setListening(true); }
    catch { setNotice("Voice input could not start. You can continue by typing."); }
  };

  const speak = () => {
    if (!consent || !canSpeak || !response) return;
    window.speechSynthesis.cancel();
    window.speechSynthesis.speak(new SpeechSynthesisUtterance(response));
  };

  return <div className="genesis-voice">
    <p>{native && !RecognitionApi ? "Voice uses local Windows dictation and your installed speech language." : "Voice uses your browser speech service, which may process audio remotely."} Abigail keeps submitted text in this creation session, not audio recordings.</p>
    <label className="genesis-check"><input type="checkbox" checked={consent} onChange={(event) => {
      setConsent(event.target.checked);
      if (!event.target.checked) { stop(); if (canSpeak) window.speechSynthesis.cancel(); }
    }} />I consent to speech input and optional spoken responses for this session.</label>
    {!RecognitionApi && !native && <p role="status">Voice input is unavailable in this browser. Use text or Markdown to continue.</p>}
    <label className="genesis-check"><input type="checkbox" checked={handsFree} disabled={!consent} onChange={(event) => setHandsFree(event.target.checked)} />Send dictated answers and read replies automatically.</label>
    <p>Voice commands: “accept suggestions”, “review entity”, “awaken entity”, “skip question”, and “save and exit”. Otherwise your words appear below for review unless automatic sending is enabled.</p>
    <div className="genesis-actions">
      <button type="button" disabled={(!RecognitionApi && !native) || !consent || (disabled && !listening)} onClick={() => void listen()}>{listening ? "Stop listening" : "Speak an answer"}</button>
      <button type="button" disabled={!canSpeak || !consent || !response} onClick={speak}>Read response aloud</button>
      {canSpeak && <button type="button" onClick={() => window.speechSynthesis.cancel()}>Stop playback</button>}
    </div>
    {listening && <p role="status">Listening… {handsFree ? "Your answer will be submitted when you finish speaking." : "Your transcript will appear in the composer for review."}{interim && <span> {interim}</span>}</p>}
    {notice && <p role="status">{notice}</p>}
  </div>;
}
