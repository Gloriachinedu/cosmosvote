import { useState, useEffect, useMemo, useRef } from 'react';
import type { Proposal, ProposalState } from './types';
import { fetchAllProposals, fetchTokenDecimals, checkRpcReachability } from './api';
import { ProposalCard } from './components/ProposalCard';
import { ProposalSkeleton } from './components/ProposalSkeleton';
import { ProposalDetail } from './components/ProposalDetail';
import { CreateProposalForm } from './components/CreateProposalForm';
import { ConnectWalletModal } from './components/ConnectWalletModal';
import { useWallet } from './WalletContext';
import { ACTIVE_NETWORK } from './config';
import { formatTokenAmount } from './utils';
import styles from './App.module.css';
import './responsive.css';

const ALL_STATES: ProposalState[] = ['Active', 'Passed', 'Rejected', 'Executed', 'Cancelled'];

// Admin address — in production this would come from the contract or environment config
const ADMIN_ADDRESS = import.meta.env.VITE_ADMIN_ADDRESS ?? null;

export default function App() {
  const { walletAddress, tokenBalance, showModal, openModal, disconnect } = useWallet();
  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [loading, setLoading] = useState(true);
  const [progress, setProgress] = useState<{ loaded: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [stateFilter, setStateFilter] = useState<ProposalState | 'All'>('All');
  const [selected, setSelected] = useState<Proposal | null>(null);
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [decimals, setDecimals] = useState<number>(0);
  const [rpcWarning, setRpcWarning] = useState<string | null>(null);
  const triggerRef = useRef<HTMLElement>(null);

  const refreshProposals = () => {
    setLoading(true);
    fetchAllProposals()
      .then(setProposals)
      .catch(e => setError(String(e)))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    Promise.all([
      fetchAllProposals((loaded, total) => setProgress({ loaded, total })),
      fetchTokenDecimals(),
    ])
      .then(([props, decs]) => {
        setProposals(props);
        setDecimals(decs);
      })
      .catch(e => setError(String(e)))
      .finally(() => { setLoading(false); setProgress(null); });
  }, []);

  useEffect(() => {
    checkRpcReachability().catch(error => {
      setRpcWarning(String(error));
    });
  }, []);

  const filtered = useMemo(() => {
    return proposals.filter(p => {
      const matchState = stateFilter === 'All' || p.state === stateFilter;
      const q = search.toLowerCase();
      const matchSearch = !q || p.title.toLowerCase().includes(q) || p.description.toLowerCase().includes(q);
      return matchState && matchSearch;
    });
  }, [proposals, search, stateFilter]);

  const handleProposalCreated = (id: number) => {
    setShowCreateForm(false);
    setSelected(null);
    // Refetch proposals to show the new one
    refreshProposals();
    // Optionally navigate or announce success
  };

  return (
    <div style={{ minHeight: '100vh', background: '#f8fafc', fontFamily: 'system-ui, sans-serif' }}>
      {/* Header */}
      <header style={{ background: 'var(--bg-header)', color: 'var(--text-header)', padding: '1rem 2rem', display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '0.75rem' }}>
        <div>
          <h1 style={{ margin: 0, fontSize: '1.5rem' }}>🌌 CosmosVote</h1>
          <span style={{ fontSize: '0.75rem', color: 'var(--text-header-sub)' }}>On-chain governance · {ACTIVE_NETWORK}</span>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem' }}>
          {walletAddress ? (
            <div style={{ textAlign: 'right' }}>
              <div style={{ fontSize: '0.75rem', color: 'var(--text-header-sub)' }}>{walletAddress.slice(0, 6)}...{walletAddress.slice(-4)}</div>
              {tokenBalance !== null && (
                <div style={{ fontSize: '0.8rem', fontWeight: 600 }}>{formatTokenAmount(tokenBalance, decimals)}</div>
              )}
              <button
                onClick={disconnect}
                style={{ marginTop: '0.25rem', background: 'none', color: '#94a3b8', border: '1px solid #475569', borderRadius: 4, padding: '0.2rem 0.5rem', cursor: 'pointer', fontSize: '0.7rem' }}
              >
                Disconnect
              </button>
            </div>
          ) : (
            <button
              onClick={openModal}
              style={{ background: '#3b82f6', color: '#fff', border: 'none', borderRadius: 6, padding: '0.5rem 1rem', cursor: 'pointer' }}
            >
              Connect Wallet
            </button>
          )}
        </div>
      </header>

      <main style={{ maxWidth: 900, margin: '0 auto', padding: '2rem 1rem' }}>
        {/* Filters + New Proposal Button */}
        <div style={{ display: 'flex', gap: '0.75rem', marginBottom: '1.5rem', flexWrap: 'wrap' }}>
          <input
            type="search"
            placeholder="Search proposals..."
            value={search}
            onChange={e => setSearch(e.target.value)}
            style={{ flex: 1, minWidth: 200, padding: '0.5rem 0.75rem', border: '1px solid var(--input-border)', borderRadius: 6, fontSize: '0.875rem', background: 'var(--bg-input)', color: 'var(--text-primary)' }}
            aria-label="Search proposals"
          />
          <select
            value={stateFilter}
            onChange={e => setStateFilter(e.target.value as ProposalState | 'All')}
            style={{ padding: '0.5rem 0.75rem', border: '1px solid var(--input-border)', borderRadius: 6, fontSize: '0.875rem', background: 'var(--bg-input)', color: 'var(--text-primary)' }}
            aria-label="Filter by state"
          >
            <option value="All">All States</option>
            {ALL_STATES.map(s => <option key={s} value={s}>{s}</option>)}
          </select>
          <button
            onClick={() => setShowCreateForm(true)}
            disabled={!walletAddress}
            title={!walletAddress ? 'Connect wallet to create proposals' : 'Create new proposal'}
            style={{
              padding: '0.5rem 1rem',
              background: walletAddress ? '#10b981' : '#d1d5db',
              color: walletAddress ? '#fff' : '#6b7280',
              border: 'none',
              borderRadius: 6,
              cursor: walletAddress ? 'pointer' : 'not-allowed',
              fontSize: '0.875rem',
              fontWeight: 600,
            }}
          >
            + New Proposal
          </button>
        </div>

        {rpcWarning && (
          <div style={{ background: '#fef3c7', color: '#92400e', border: '1px solid #fde68a', borderRadius: 8, padding: '1rem', marginBottom: '1rem' }}>
            <strong>RPC warning:</strong> {rpcWarning}
          </div>
        )}

        {/* Stats bar */}
        <div style={{ display: 'flex', gap: '1rem', marginBottom: '1.5rem', flexWrap: 'wrap' }}>
          {[
            { label: 'Total', count: proposals.length, color: 'var(--text-primary)' },
            { label: 'Active', count: proposals.filter(p => p.state === 'Active').length, color: '#2563eb' },
            { label: 'Passed', count: proposals.filter(p => p.state === 'Passed').length, color: '#16a34a' },
            { label: 'Executed', count: proposals.filter(p => p.state === 'Executed').length, color: '#7c3aed' },
          ].map(({ label, count, color }) => (
            <div key={label} style={{ background: 'var(--bg-stat)', border: '1px solid var(--border-color)', borderRadius: 8, padding: '0.5rem 1rem', textAlign: 'center' }}>
              <div style={{ fontSize: '1.25rem', fontWeight: 700, color }}>{count}</div>
              <div style={{ fontSize: '0.75rem', color: 'var(--text-muted)' }}>{label}</div>
            </div>
          ))}
        </div>

        {/* Content */}
        {error && <p style={{ textAlign: 'center', color: '#dc2626', marginBottom: '1rem' }}>Error: {error}</p>}

        <div style={{ display: 'grid', gap: '1rem' }}>
          {loading && (
            <>
              {progress && (
                <p style={{ textAlign: 'center', color: '#64748b', fontSize: '0.875rem' }}>
                  Loading proposals… {progress.loaded}/{progress.total}
                </p>
              )}
              <ProposalSkeleton />
              <ProposalSkeleton />
              <ProposalSkeleton />
            </>
          )}
          {!loading && !error && filtered.length === 0 && (
            <p style={{ textAlign: 'center', color: 'var(--text-muted)' }}>No proposals found.</p>
          )}
          {!loading && filtered.map(p => (
            <ProposalCard key={String(p.id)} proposal={p} onClick={(e) => {
              triggerRef.current = e?.currentTarget as HTMLElement ?? null;
              setSelected(p);
            }} />
          ))}
        </div>
      </main>

      {/* Modals */}
      {selected && (
        <ProposalDetail
          proposal={selected}
          decimals={decimals}
          walletAddress={walletAddress}
          adminAddress={ADMIN_ADDRESS}
          onClose={() => setSelected(null)}
          triggerRef={triggerRef}
        />
      )}

      {showCreateForm && (
        <CreateProposalForm
          onClose={() => setShowCreateForm(false)}
          onSuccess={handleProposalCreated}
        />
      )}

      {showModal && <ConnectWalletModal />}
    </div>
  );
}
