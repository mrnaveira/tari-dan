//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import { useEffect } from 'react';
import { Form, useLoaderData } from 'react-router-dom';
import { getSubstates, getTransaction, getCurrentLeaderState } from '../../utils/json_rpc';
import {
  fromHexString,
  toHexString,
  shortenString,
} from '../VN/Components/helpers';
import Output from './Components/Output';
import Substates from './Components/Substates';
import './Transaction.css';
import mermaid from 'mermaid';
import { StyledPaper } from '../../Components/StyledComponents';
import PageHeading from '../../Components/PageHeading';
import Typography from '@mui/material/Typography';
import Grid from '@mui/material/Grid';
import SecondaryHeading from '../../Components/SecondaryHeading';
import Table from '@mui/material/Table';
import TableBody from '@mui/material/TableBody';
import TableCell from '@mui/material/TableCell';
import TableContainer from '@mui/material/TableContainer';
import TableHead from '@mui/material/TableHead';
import TableRow from '@mui/material/TableRow';
import TablePagination from '@mui/material/TablePagination';

type loaderData = [string, Map<string, any[]>, Map<string, any[]>, Map<string, [string, number, string]>];

mermaid.initialize({
  startOnLoad: true,
});

function Mermaid(props: { chart: string }) {
  useEffect(() => {
    console.log(mermaid);
    mermaid.contentLoaded();
  });
  console.log(props.chart);
  return <pre className="mermaid">{props.chart}</pre>;
}

function splitToOutputs(transactions: any[]) {
  let shard_transactions = new Map<string, any[]>();
  for (let transaction of transactions) {
    let shard = toHexString(transaction.shard);
    if (!shard_transactions.has(shard)) {
      shard_transactions.set(shard, []);
    }
    shard_transactions.get(shard)?.push(transaction);
  }
  return shard_transactions;
}

function splitToShards(current_leader_states: any[]) {
  let states = new Map<string, [string, number, string]>();
  for (let current_leader_state of current_leader_states) {
    let shard = toHexString(current_leader_state.shard_id);
    states.set(shard, [toHexString(current_leader_state.leader), current_leader_state.leader_round,current_leader_state.timestamp]);
  }
  return states;
}

export async function transactionLoader({ params }: { params: any }) {
  const outputs = splitToOutputs(await getTransaction(params.payloadId));
  const current_leader_states = splitToShards(await getCurrentLeaderState(params.payloadId));
  let substates = new Map<string, any[]>();

  await Promise.all(
    Array.from(outputs.entries()).map(async ([shard, _]) => {
      substates.set(shard, await getSubstates(params.payloadId, shard));
    })
  );
  return [params.payloadId, substates, outputs, current_leader_states];
}

function mapHeight(height: number) {
  switch (height) {
    case 1:
      return 'Prepare';
    case 2:
      return 'Precommit';
    case 3:
      return 'Commit';
    case 4:
      return 'Decide';
    default:
      return 'Unknown';
  }
}

export default function Transaction() {
const [payloadId, substates, outputs, current_leader_states] = useLoaderData() as loaderData;
  console.log('Substates: ', substates);
  console.log('Outputs: ', outputs);
  console.log('Current states: ', current_leader_states);
  let mermaid = 'gantt\ndateFormat YYYY-MM-DDTHH:mm:ss\naxisFormat  %Hh%M:%S';
  let shardNo = 0;
  for (let [shard, output] of Array.from(outputs.entries())) {
    mermaid += `\nsection shard_${shardNo}`;
    for (let node of output) {
      let justify = JSON.parse(node.justify);
      mermaid += `\n[QC - ${
        justify.local_node_height === 0
          ? 'Genesis'
          : justify.decision.Reject || justify.decision
      } ${
        justify.local_node_height === 0
          ? ''
          : ' w ' + justify.validators_metadata.length + ' votes'
      }] ${mapHeight(node.height)}  :done, s${shardNo}h${node.height}, ${
        node.timestamp
      } , 1s`;
    }
    shardNo++;
  }
  mermaid += '\n';

  return (
    <>
      <Grid container spacing={5}>
        <Grid item xs={12} md={12} lg={12}>
          <PageHeading>Payload ID</PageHeading>
          <Typography variant="h5" sx={{ mt: 4, mb: 4, textAlign: 'center' }}>
            {payloadId}
          </Typography>
        </Grid>
        <Grid item xs={12} md={12} lg={12}>
          <StyledPaper>
            <Typography>
              <strong>Outputs :</strong> {outputs?.size}
            </Typography>
            <Mermaid chart={mermaid} />
          </StyledPaper>
        </Grid>
        <SecondaryHeading>Substates</SecondaryHeading>
        <Grid item xs={12} md={12} lg={12}>
          <StyledPaper>
            <TableContainer>
              <Table>
                <TableHead>
                  <TableRow>
                    <TableCell>Shard</TableCell>
                    <TableCell>Address</TableCell>
                    <TableCell style={{ textAlign: 'center', width: '120px' }}>
                      Data
                    </TableCell>
                    <TableCell style={{ textAlign: 'center', width: '120px' }}>
                      Created
                    </TableCell>
                    <TableCell style={{ textAlign: 'center', width: '120px' }}>
                      Destroyed
                    </TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {Array.from(outputs.entries()).map(([shard]) => (
                    <Substates substates={substates.get(shard)} />
                  ))}
                </TableBody>
              </Table>
            </TableContainer>
          </StyledPaper>
        </Grid>
        <SecondaryHeading>Shards</SecondaryHeading>
        {Array.from(outputs.entries()).map(([shard, output]) => (
          <>
            <Grid item xs={12} md={12} lg={12}>
              <StyledPaper>
                <Output key={shard} shard={shard} output={output} current_state={current_leader_states.get(shard)}/>
              </StyledPaper>
            </Grid>
          </>
        ))}
      </Grid>
    </>
  );
}
