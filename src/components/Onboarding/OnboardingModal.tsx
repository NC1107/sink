import { useEffect, useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { Ms } from "../Icons";

interface Step {
  icon: string;
  title: string;
  body: string;
  /** Show the signal-flow diagram under the body. */
  diagram?: boolean;
}

/** The signal-flow picture: apps → channels → ears, with mixes tapping
 * channels for recorders. Styled like the mixer itself (channel = indigo,
 * mix = amber). */
function FlowDiagram() {
  return (
    <div className="ob-flow">
      <div className="ob-flow-row">
        <span className="ob-flow-node">
          <Ms name="apps" />
          Apps
        </span>
        <Ms name="arrow_forward" className="ob-flow-arrow" />
        <span className="ob-flow-node is-channel">
          <Ms name="tune" />
          Channels
        </span>
        <Ms name="arrow_forward" className="ob-flow-arrow" />
        <span className="ob-flow-node">
          <Ms name="headphones" />
          Your ears
        </span>
      </div>
      <div className="ob-flow-row ob-flow-branch">
        <Ms name="subdirectory_arrow_right" className="ob-flow-arrow" />
        <span className="ob-flow-node is-mix">
          <Ms name="radio_button_checked" />
          Mixes
        </span>
        <Ms name="arrow_forward" className="ob-flow-arrow" />
        <span className="ob-flow-node">
          <Ms name="videocam" />
          OBS / recorder
        </span>
      </div>
    </div>
  );
}

// Three short cards: the mental model first (as a picture), then the two
// things you can't discover just by looking at the screen.
const STEPS: Step[] = [
  {
    icon: "graphic_eq",
    title: "Your sound, on a board",
    body: "Every app's audio lands on a channel you control. Send each channel to your ears - and tap any group as a recording.",
    diagram: true,
  },
  {
    icon: "grid_view",
    title: "Sort your apps",
    body: "New apps appear on their own. Drop each onto a channel - game, chat, music - and Sink keeps it there next time.",
  },
  {
    icon: "mic",
    title: "Microphone",
    body: "Optional: run your mic through a noise gate, compressor and limiter, then pick the result in Discord or OBS.",
  },
];

/** Progress dots: one per concept card plus the final choice page. */
function ObDots({ step }: Readonly<{ step: number }>) {
  return (
    <div className="ob-dots">
      {[...STEPS, null].map((s, i) => (
        <span key={s?.title ?? "choice"} className={"ob-dot" + (i === step ? " on" : "")} />
      ))}
    </div>
  );
}

/** First-run tutorial: one card per concept, then a starting-point choice. */
export function OnboardingModal() {
  const show = useMixerStore((s) => s.showOnboarding);
  const replay = useMixerStore((s) => s.onboardingReplay);
  const finishOnboarding = useMixerStore((s) => s.finishOnboarding);
  const [step, setStep] = useState(0);

  // Replays start from the first card again.
  useEffect(() => {
    if (show) setStep(0);
  }, [show]);

  if (!show) return null;

  const last = step === STEPS.length; // the choice page
  const current = STEPS[step];

  let body;
  if (last && replay) {
    body = (
      <>
        <div className="modal-title">That's the tour</div>
        <p className="modal-text">
          Channels, apps and the mic are all live - your setup is untouched.
        </p>
        <div className="ob-foot">
          <button className="modal-btn" onClick={() => setStep(step - 1)}>
            Back
          </button>
          <ObDots step={step} />
          <button className="modal-btn primary" onClick={() => void finishOnboarding(false)}>
            Done
          </button>
        </div>
      </>
    );
  } else if (last) {
    body = (
      <>
        <div className="modal-title">How do you want to start?</div>
        <p className="modal-text">
          Either way you can add, rename or delete channels whenever - this just
          lays out your first board.
        </p>
        <div className="ob-choices">
          <button className="ob-choice" onClick={() => void finishOnboarding(false)}>
            <Ms name="dashboard" />
            <div className="ob-choice-title">Set up a board for me</div>
            <div className="ob-choice-sub">
              Game, Chat, Music and System - ready to drop apps onto
            </div>
          </button>
          <button className="ob-choice" onClick={() => void finishOnboarding(true)}>
            <Ms name="check_box_outline_blank" />
            <div className="ob-choice-title">I'll build my own</div>
            <div className="ob-choice-sub">One Main channel - add the rest as you go</div>
          </button>
        </div>
        <div className="ob-foot">
          <button className="modal-btn" onClick={() => setStep(step - 1)}>
            Back
          </button>
          <ObDots step={step} />
          <span style={{ width: 64 }} />
        </div>
      </>
    );
  } else {
    body = (
      <>
        <div className="ob-icon">
          <Ms name={current.icon} />
        </div>
        <div className="modal-title" style={{ textAlign: "center" }}>
          {current.title}
        </div>
        <p className="modal-text ob-body">{current.body}</p>
        {current.diagram && <FlowDiagram />}
        <div className="ob-foot">
          {step > 0 ? (
            <button className="modal-btn" onClick={() => setStep(step - 1)}>
              Back
            </button>
          ) : (
            <button className="modal-btn" onClick={() => void finishOnboarding(false)}>
              Skip
            </button>
          )}
          <ObDots step={step} />
          <button className="modal-btn primary" onClick={() => setStep(step + 1)}>
            Next
          </button>
        </div>
      </>
    );
  }

  return (
    <div className="modal-scrim">
      <div className="modal ob-modal" role="dialog" aria-label="Welcome to Sink">
        {body}
      </div>
    </div>
  );
}
