# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

@dan
Feature: Validator Node

    @critical
    Scenario: Get identity via JSON-RPC
        Given I have a seed node NODE1
        And I have wallet WALLET1 connected to all seed nodes
        And I have a validator node VN1 connected to base node NODE1 and wallet WALLET1
        Given I call "get_identity" on VN1 via JSON-RPC with params "[]"
        Then the JSON-RPC response should be:
            """
            {   
                "node_id":"5d4a78a0e3fe5ff8dfd16e864f",
                "public_key":"608001dffed28d058591cd65eaca11c465165592baf872cf1d984e26fb12b472",
                "public_address":""
            }
            """

    @critical
    Scenario: Submit transaction via JSON-RPC
        Given I have a seed node NODE1
        And I have wallet WALLET1 connected to all seed nodes
        And I have a validator node VN1 connected to base node NODE1 and wallet WALLET1
        Given I call "submit_transaction" on VN1 via JSON-RPC with a valid transaction
        # TODO: when the transaction processing is implemented, assert the real response
        Then the JSON-RPC response should be:
            """
            null
            """