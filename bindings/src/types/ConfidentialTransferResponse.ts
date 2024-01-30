// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.
import type { Amount } from "./Amount";
import type { FinalizeResult } from "./FinalizeResult";
import type { TransactionId } from "./TransactionId";

export interface ConfidentialTransferResponse {
  transaction_id: TransactionId;
  fee: Amount;
  result: FinalizeResult;
}
