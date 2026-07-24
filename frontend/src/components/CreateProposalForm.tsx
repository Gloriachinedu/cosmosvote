import { useState, useRef, useEffect } from 'react';
import { useWallet } from '../WalletContext';
import { createProposal } from '../api';
import { ERROR_MESSAGES } from '../errors';
import styles from './CreateProposalForm.module.css';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface FormState {
  title: string;
  description: string;
  quorum: string;
  duration: string;
  link: string;
}

type FieldErrors = Partial<Record<keyof FormState, string>>;

const INITIAL: FormState = {
  title: '',
  description: '',
  quorum: '',
  duration: '',
  link: '',
};

// ---------------------------------------------------------------------------
// Validation — mirrors contract constraints exactly
// ---------------------------------------------------------------------------

function validate(f: FormState): FieldErrors {
  const errs: FieldErrors = {};

  if (!f.title.trim()) {
    errs.title = 'Title is required.';
  } else if (f.title.length > 128) {
    errs.title = 'Title must be 1–128 characters.';
  }

  if (!f.description.trim()) {
    errs.description = 'Description is required.';
  } else if (f.description.length > 1024) {
    errs.description = 'Description must be 1–1024 characters.';
  }

  const q = Number(f.quorum);
  if (!f.quorum.trim() || isNaN(q) || q <= 0 || !Number.isInteger(q)) {
    errs.quorum = 'Quorum must be a positive whole number (token units).';
  }

  const d = Number(f.duration);
  if (!f.duration.trim() || isNaN(d) || d < 60 || d > 2_592_000 || !Number.isInteger(d)) {
    errs.duration = 'Duration must be between 60 and 2,592,000 seconds (1 min – 30 days).';
  }

  if (f.link.trim() !== '') {
    if (f.link.trim().length > 256) {
      errs.link = 'Link must be 256 characters or fewer.';
    } else {
      try {
        new URL(f.link.trim());
      } catch {
        errs.link = 'Link must be a valid URL (e.g. https://example.com).';
      }
    }
  }

  return errs;
}

// ---------------------------------------------------------------------------
// Contract error decoder
// ---------------------------------------------------------------------------

function decodeContractError(raw: unknown): string {
  const msg = String(raw);

  // Try to extract the numeric error code from Soroban error strings
  // e.g. "Error(Contract, #15)" or "contract error 15"
  const codeMatch = msg.match(/#(\d+)|contract\s+error\s+(\d+)/i);
  if (codeMatch) {
    const code = Number(codeMatch[1] ?? codeMatch[2]);
    const entry = Object.values(ERROR_MESSAGES).find(e => e.code === code);
    if (entry) {
      return `${entry.name}: ${entry.description}`;
    }
    return `Contract error #${code}.`;
  }

  // Common human-readable cases
  if (msg.toLowerCase().includes('freighter')) return msg;
  if (msg.toLowerCase().includes('user declined')) return 'Transaction was cancelled in Freighter.';
  if (msg.toLowerCase().includes('not connected')) return 'Freighter wallet is not connected.';
  if (msg.toLowerCase().includes('simulation')) return `Simulation failed: ${msg}`;

  return msg;
}

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface Props {
  onClose: () => void;
  /** Called with the new proposal ID after successful submission. */
  onSuccess: (proposalId: number) => void;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function CreateProposalForm({ onClose, onSuccess }: Props) {
  const { walletAddress, openModal } = useWallet();
  const [form, setForm] = useState<FormState>(INITIAL);
  const [errors, setErrors] = useState<FieldErrors>({});
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [successId, setSuccessId] = useState<number | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const firstFieldRef = useRef<HTMLInputElement>(null);

  // Focus the first field when the dialog opens
  useEffect(() => {
    firstFieldRef.current?.focus();
  }, []);

  const set = (k: keyof FormState) =>
    (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      setForm(f => ({ ...f, [k]: e.target.value }));
      // Clear error on change so the user gets instant feedback
      if (errors[k]) setErrors(prev => ({ ...prev, [k]: undefined }));
    };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const errs = validate(form);
    if (Object.keys(errs).length > 0) {
      setErrors(errs);
      return;
    }
    setErrors({});
    setSubmitError(null);
    setSubmitting(true);

    try {
      const id = await createProposal({
        proposer: walletAddress!,
        title: form.title.trim(),
        description: form.description.trim(),
        quorum: BigInt(Math.round(Number(form.quorum))),
        duration: Math.round(Number(form.duration)),
        link: form.link.trim() || undefined,
      });
      setSuccessId(id);
      onSuccess(id);
    } catch (err) {
      setSubmitError(decodeContractError(err));
    } finally {
      setSubmitting(false);
    }
  };

  const titleLen = form.title.length;
  const descLen = form.description.length;

  return (
    <div
      className={styles.overlay}
      onClick={onClose}
      role="presentation"
    >
      <div
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="cpf-title"
        onClick={e => e.stopPropagation()}
      >
        {/* Header */}
        <div className={styles.header}>
          <h2 id="cpf-title" className={styles.title}>New Proposal</h2>
          <button
            type="button"
            onClick={onClose}
            className={styles.closeBtn}
            aria-label="Close proposal form"
          >
            ×
          </button>
        </div>

        {/* Wallet not connected warning */}
        {!walletAddress && (
          <div className={styles.walletBanner} role="alert">
            Connect your wallet to submit a proposal.{' '}
            <button
              type="button"
              onClick={openModal}
              style={{ textDecoration: 'underline', background: 'none', border: 'none', cursor: 'pointer', color: 'inherit', fontWeight: 600 }}
            >
              Connect wallet
            </button>
          </div>
        )}

        {/* Success banner */}
        {successId !== null && (
          <div className={styles.successBanner} role="status" aria-live="polite">
            ✅ Proposal #{successId} created successfully!
          </div>
        )}

        {/* Submit error banner */}
        {submitError && (
          <div className={styles.errorBanner} role="alert" aria-live="assertive">
            <strong>Error:</strong> {submitError}
          </div>
        )}

        <form onSubmit={handleSubmit} noValidate aria-describedby={!walletAddress ? 'cpf-wallet-warning' : undefined}>
          {/* ── Title ── */}
          <div className={styles.field}>
            <label htmlFor="cpf-title-input" className={styles.label}>
              Title <span className={styles.required} aria-hidden="true">*</span>
            </label>
            <input
              id="cpf-title-input"
              ref={firstFieldRef}
              type="text"
              value={form.title}
              onChange={set('title')}
              maxLength={128}
              className={styles.input}
              aria-required="true"
              aria-describedby={errors.title ? 'cpf-title-err' : 'cpf-title-hint'}
              aria-invalid={!!errors.title}
              disabled={!walletAddress || submitting}
              placeholder="Short, descriptive proposal title"
            />
            <span id="cpf-title-hint" className={titleLen > 110 ? styles.charCountWarn : styles.charCount}>
              {titleLen}/128
            </span>
            {errors.title && (
              <span id="cpf-title-err" className={styles.fieldError} role="alert">{errors.title}</span>
            )}
          </div>

          {/* ── Description ── */}
          <div className={styles.field}>
            <label htmlFor="cpf-desc" className={styles.label}>
              Description <span className={styles.required} aria-hidden="true">*</span>
            </label>
            <textarea
              id="cpf-desc"
              value={form.description}
              onChange={set('description')}
              maxLength={1024}
              rows={5}
              className={styles.textarea}
              aria-required="true"
              aria-describedby={errors.description ? 'cpf-desc-err' : 'cpf-desc-hint'}
              aria-invalid={!!errors.description}
              disabled={!walletAddress || submitting}
              placeholder="Full rationale and context for the proposal"
            />
            <span id="cpf-desc-hint" className={descLen > 900 ? styles.charCountWarn : styles.charCount}>
              {descLen}/1024
            </span>
            {errors.description && (
              <span id="cpf-desc-err" className={styles.fieldError} role="alert">{errors.description}</span>
            )}
          </div>

          {/* ── Quorum ── */}
          <div className={styles.field}>
            <label htmlFor="cpf-quorum" className={styles.label}>
              Quorum <span className={styles.required} aria-hidden="true">*</span>
            </label>
            <input
              id="cpf-quorum"
              type="number"
              min={1}
              step={1}
              value={form.quorum}
              onChange={set('quorum')}
              className={styles.input}
              aria-required="true"
              aria-describedby={errors.quorum ? 'cpf-quorum-err' : 'cpf-quorum-hint'}
              aria-invalid={!!errors.quorum}
              disabled={!walletAddress || submitting}
              placeholder="Minimum token units required to pass"
            />
            <span id="cpf-quorum-hint" className={styles.hint}>
              Token units (whole numbers only). Must be &gt; 0 and ≤ total supply.
            </span>
            {errors.quorum && (
              <span id="cpf-quorum-err" className={styles.fieldError} role="alert">{errors.quorum}</span>
            )}
          </div>

          {/* ── Duration ── */}
          <div className={styles.field}>
            <label htmlFor="cpf-duration" className={styles.label}>
              Voting Duration <span className={styles.required} aria-hidden="true">*</span>
            </label>
            <input
              id="cpf-duration"
              type="number"
              min={60}
              max={2592000}
              step={1}
              value={form.duration}
              onChange={set('duration')}
              className={styles.input}
              aria-required="true"
              aria-describedby={errors.duration ? 'cpf-duration-err' : 'cpf-duration-hint'}
              aria-invalid={!!errors.duration}
              disabled={!walletAddress || submitting}
              placeholder="e.g. 86400 for 1 day"
            />
            <span id="cpf-duration-hint" className={styles.hint}>
              Seconds — min 60 (1 min), max 2,592,000 (30 days).
            </span>
            {errors.duration && (
              <span id="cpf-duration-err" className={styles.fieldError} role="alert">{errors.duration}</span>
            )}
          </div>

          {/* ── Optional fields section ── */}
          <div className={styles.sectionLabel} aria-hidden="true">Optional fields</div>

          {/* ── Link ── */}
          <div className={styles.field}>
            <label htmlFor="cpf-link" className={styles.label}>Discussion Link</label>
            <input
              id="cpf-link"
              type="url"
              value={form.link}
              onChange={set('link')}
              maxLength={256}
              className={styles.input}
              aria-describedby={errors.link ? 'cpf-link-err' : 'cpf-link-hint'}
              aria-invalid={!!errors.link}
              disabled={!walletAddress || submitting}
              placeholder="https://forum.example.com/proposal-discussion"
            />
            <span id="cpf-link-hint" className={styles.hint}>
              External forum or governance discussion thread. Max 256 characters.
            </span>
            {errors.link && (
              <span id="cpf-link-err" className={styles.fieldError} role="alert">{errors.link}</span>
            )}
          </div>

          {/* ── Actions ── */}
          <div className={styles.actions}>
            <button
              type="button"
              onClick={onClose}
              className={styles.cancelBtn}
              disabled={submitting}
            >
              Cancel
            </button>
            <button
              type="submit"
              className={styles.submitBtn}
              disabled={!walletAddress || submitting}
              aria-busy={submitting}
            >
              {submitting && <span className={styles.spinner} aria-hidden="true" />}
              {submitting ? 'Submitting…' : 'Submit Proposal'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
