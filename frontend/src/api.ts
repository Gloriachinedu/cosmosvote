import {
  SorobanRpc,
  TransactionBuilder,
  Account,
  Operation,
  xdr,
  scValToNative,
  nativeToScVal,
} from '@stellar/stellar-sdk';
import { config } from './config';
import type { Proposal, VoteRecord } from './types';

const server = new SorobanRpc.Server(config.rpcUrl);

// Simulate a read-only contract call without a real account
async function simulateCall(
  contractId: string,
  method: string,
  ...args: xdr.ScVal[]
): Promise<unknown> {
  // Use a zero-sequence dummy account — valid for simulation only
  const dummyAccount = new Account(
    'GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN',
    '0'
  );

  const tx = new TransactionBuilder(dummyAccount, {
    fee: '100',
    networkPassphrase: config.networkPassphrase,
  })
    .addOperation(
      Operation.invokeContractFunction({
        contract: contractId,
        function: method,
        args,
      })
    )
    .setTimeout(30)
    .build();

  const result = (await server.simulateTransaction(
    tx
  )) as SorobanRpc.Api.SimulateTransactionSuccessResponse;

  if (!result.result) throw new Error(`Simulation failed for ${method}`);
  return scValToNative(result.result.retval);
}

export async function checkRpcReachability(): Promise<void> {
  try {
    const response = await fetch(config.rpcUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'getHealth', params: [] }),
    });

    if (!response.ok) {
      throw new Error(`RPC returned HTTP ${response.status}`);
    }

    const body = await response.json();
    if (body.error) {
      throw new Error(`RPC error: ${JSON.stringify(body.error)}`);
    }
  } catch (err) {
    throw new Error(
      `Unable to reach Soroban RPC at ${config.rpcUrl}. Confirm the RPC endpoint is running and that CORS allows browser access. ${
        err instanceof Error ? err.message : String(err)
      }`
    );
  }
}

export async function fetchProposalCount(): Promise<number> {
  const count = await simulateCall(config.governanceContractId, 'proposal_count');
  return Number(count);
}

export async function fetchProposal(id: number): Promise<Proposal> {
  const raw = await simulateCall(
    config.governanceContractId,
    'get_proposal',
    nativeToScVal(BigInt(id), { type: 'u64' })
  );
  return raw as Proposal;
}

export async function fetchAllProposals(
  onProgress?: (loaded: number, total: number) => void
): Promise<Proposal[]> {
  const count = await fetchProposalCount();
  const results = await Promise.allSettled(
    Array.from({ length: count }, (_, i) => fetchProposal(i))
  );
  return results
    .filter((r): r is PromiseFulfilledResult<Proposal> => {
      if (r.status === 'rejected') {
        console.error('[fetchAllProposals] failed to fetch proposal:', r.reason);
        return false;
      }
      return true;
    })
    .map(r => r.value);
}

export async function fetchHasVoted(proposalId: number, voter: string): Promise<boolean> {
  const result = await simulateCall(
    config.governanceContractId,
    'has_voted',
    nativeToScVal(BigInt(proposalId), { type: 'u64' }),
    nativeToScVal(voter, { type: 'address' })
  );
  return Boolean(result);
}

export async function fetchVoteRecord(
  proposalId: number,
  voter: string
): Promise<VoteRecord | null> {
  try {
    const result = await simulateCall(
      config.governanceContractId,
      'get_vote',
      nativeToScVal(BigInt(proposalId), { type: 'u64' }),
      nativeToScVal(voter, { type: 'address' })
    );
    return result as VoteRecord;
  } catch {
    return null;
  }
}

export async function fetchTokenBalance(address: string): Promise<bigint> {
  const result = await simulateCall(
    config.tokenContractId,
    'balance',
    nativeToScVal(address, { type: 'address' })
  );
  return BigInt(String(result));
}

export async function fetchTokenDecimals(): Promise<number> {
  const result = await simulateCall(
    config.tokenContractId,
    'decimals'
  );
  return Number(result);
}

export async function fetchActiveProposalCount(): Promise<number> {
  const result = await simulateCall(
    config.governanceContractId,
    'active_proposal_count'
  );
  return Number(result);
}

export interface CreateProposalParams {
  proposer: string;
  title: string;
  description: string;
  quorum: bigint;
  duration: number;
  link?: string;
}

/**
 * Build, simulate, sign (via Freighter), and submit a create_proposal transaction.
 * Returns the new proposal ID on success.
 */
export async function createProposal(params: CreateProposalParams): Promise<number> {
  const { isConnected, requestAccess, getAddress, signTransaction } = await import('@stellar/freighter-api');

  const connected = await isConnected();
  if (!connected) throw new Error('Freighter extension not found. Please install it.');

  await requestAccess();
  const addrResult = await getAddress();
  if (addrResult.error) throw new Error(addrResult.error.message);

  const walletAddress = addrResult.address;
  const accountResp = await server.getAccount(walletAddress);
  const account = new Account(walletAddress, accountResp.sequence);

  const linkArg = params.link
    ? nativeToScVal(params.link, { type: 'string' })
    : xdr.ScVal.scvVoid();

  const tx = new TransactionBuilder(account, {
    fee: '100',
    networkPassphrase: config.networkPassphrase,
  })
    .addOperation(
      Operation.invokeContractFunction({
        contract: config.governanceContractId,
        function: 'create_proposal',
        args: [
          nativeToScVal(walletAddress, { type: 'address' }),
          nativeToScVal(params.title, { type: 'string' }),
          nativeToScVal(params.description, { type: 'string' }),
          nativeToScVal(params.quorum, { type: 'i128' }),
          nativeToScVal(BigInt(params.duration), { type: 'u64' }),
          linkArg,
          xdr.ScVal.scvVoid(), // treasury_action: None
        ],
      })
    )
    .setTimeout(30)
    .build();

  // Simulate to verify and obtain the authorisation + footprint
  const simResult = await server.simulateTransaction(tx) as SorobanRpc.Api.SimulateTransactionSuccessResponse;
  if ('error' in simResult || !simResult.result) {
    throw new Error(
      'error' in simResult
        ? String((simResult as unknown as { error: string }).error)
        : 'Transaction simulation failed. Check contract address and inputs.'
    );
  }

  // Assemble the transaction: attaches the simulated auth + resource fee
  const assembled = SorobanRpc.assembleTransaction(tx, simResult).build();

  // Sign via Freighter
  const signResult = await signTransaction(assembled.toXDR(), {
    networkPassphrase: config.networkPassphrase,
  });
  if ('error' in signResult) throw new Error(String(signResult.error));

  // Submit to the network
  const signed = TransactionBuilder.fromXDR(
    signResult.signedTxXdr,
    config.networkPassphrase
  );
  const sendResult = await server.sendTransaction(signed);

  if (sendResult.status === 'ERROR') {
    throw new Error(`Transaction rejected: ${JSON.stringify(sendResult.errorResult)}`);
  }

  // Poll until confirmed (up to ~30 s)
  let getResult: SorobanRpc.Api.GetTransactionResponse | undefined;
  for (let i = 0; i < 20; i++) {
    await new Promise(r => setTimeout(r, 1500));
    getResult = await server.getTransaction(sendResult.hash);
    if (getResult.status !== 'NOT_FOUND') break;
  }

  if (!getResult || getResult.status !== 'SUCCESS') {
    throw new Error(`Transaction did not confirm in time. Status: ${getResult?.status}`);
  }

  // The contract returns the new proposal ID (u64)
  const successResult = getResult as SorobanRpc.Api.GetSuccessfulTransactionResponse;
  if (successResult.returnValue) {
    return Number(scValToNative(successResult.returnValue));
  }
  return -1;
}

export async function castVote(
  walletAddress: string,
  proposalId: number,
  vote: 'Yes' | 'No' | 'Abstain'
): Promise<string> {
  // Create a dummy account for signing (in a real app, this would use the user's actual wallet)
  const dummyAccount = new Account(walletAddress, '0');

  // Convert vote string to enum value
  const voteEnum = { Yes: 0, No: 1, Abstain: 2 }[vote];

  // Build the transaction
  const tx = new TransactionBuilder(dummyAccount, {
    fee: '100',
    networkPassphrase: config.networkPassphrase,
  })
    .addOperation(
      Operation.invokeContractFunction({
        contract: config.governanceContractId,
        function: 'cast_vote',
        args: [
          nativeToScVal(walletAddress, { type: 'address' }),
          nativeToScVal(BigInt(proposalId), { type: 'u64' }),
          nativeToScVal(voteEnum, { type: 'u32' }),
        ],
      })
    )
    .setTimeout(30)
    .build();

  // Simulate the transaction to verify it works
  const result = (await server.simulateTransaction(
    tx
  )) as SorobanRpc.Api.SimulateTransactionSuccessResponse;

  if (!result.result) throw new Error('Transaction simulation failed');

  // In a real app, we would sign and submit the transaction here using the actual wallet
  // For now, we return a simulated transaction hash
  return `${result.result.retval}`;
}
