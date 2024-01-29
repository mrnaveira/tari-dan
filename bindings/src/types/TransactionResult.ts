// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.
import type { RejectReason } from "./RejectReason";
import type { SubstateDiff } from "./SubstateDiff";

export type TransactionResult =
  | { Accept: SubstateDiff }
  | { AcceptFeeRejectRest: [SubstateDiff, RejectReason] }
  | { Reject: RejectReason };
