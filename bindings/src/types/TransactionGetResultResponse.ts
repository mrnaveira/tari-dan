// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.
import type { FinalizeResult } from "./FinalizeResult";
import type { TransactionId } from "./TransactionId";
import type { TransactionStatus } from "./TransactionStatus";

export interface TransactionGetResultResponse {
  transaction_id: TransactionId;
  status: TransactionStatus;
  result: FinalizeResult | null;
  json_result: Array<any> | null;
}
