import { useState } from "react";

/** The Overlay's URL with a Copy button, and how to add it to OBS. */
export function ObsOverlay({ url, detailed = false }: { url: string; detailed?: boolean }) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    await navigator.clipboard.writeText(url);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <>
      {detailed ? (
        <ol className="obs-steps">
          <li>
            In OBS, under <strong>Sources</strong>, press <strong>+</strong> and choose{" "}
            <strong>Browser</strong>. Name it "Practice Chat".
          </li>
          <li>
            Paste this URL into <strong>URL</strong>, set the width to 400 and the height to 600,
            and press OK.
          </li>
          <li>Drag it where you want the chat. Its background is transparent.</li>
        </ol>
      ) : (
        <p>
          In OBS, add a <strong>Browser</strong> source with this URL. Its background is
          transparent.
        </p>
      )}
      <div className="overlay-url">
        <code>{url}</code>
        <button onClick={copy}>{copied ? "Copied" : "Copy"}</button>
      </div>
      {detailed && (
        <p className="hint">
          Streamlabs and other apps work too: add a browser source with the same URL. The Overlay
          only works while Small Streamer Chat is running.
        </p>
      )}
    </>
  );
}
