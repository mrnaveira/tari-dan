// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.
import type { Amount } from "./Amount";
import type { Epoch } from "./Epoch";

export interface GetValidatorFeesResponse {
  fee_summary: Record<Epoch, Amount>;
}