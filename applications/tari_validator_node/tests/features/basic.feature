# Copyright 2022 The Tari Project
# SPDX-License-Identifier: BSD-3-Clause

Feature: Basic scenarios
  @serial
  Scenario: Template registration and invocation in a 2-VN committee
    # Initialize a base node, wallet and miner
    Given a base node BASE
    Given a wallet WALLET connected to base node BASE
    Given a miner MINER connected to base node BASE and wallet WALLET

    # Initialize two validator nodes
    Given a validator node VAL_1 connected to base node BASE and wallet WALLET
    Given a validator node VAL_2 connected to base node BASE and wallet WALLET
    Then the validator node VAL_1 returns a valid identity
    Then the validator node VAL_2 returns a valid identity

    # The wallet must have some funds before the VN sends transactions
    When miner MINER mines 12 new blocks

    # VN registration
    When validator node VAL_1 sends a registration transaction
    When validator node VAL_2 sends a registration transaction
    When miner MINER mines 20 new blocks
    # FIXME: the following instructions fail due to the VNs not listed in the base node
    #        but the base node blocks do include the registration transactions
    # Then the validator node VAL_1 is listed as registered
    # Then the validator node VAL_2 is listed as registered

    # Register the "counter" template
    When validator node VAL_1 registers the template "counter"
    When miner MINER mines 20 new blocks
    Then the template "counter" is listed as registered by the validator node VAL_1
    Then the template "counter" is listed as registered by the validator node VAL_2

    # Call the constructor in the "counter" template
    # FIXME: the following instruction fails due to no epoch found
    # Then the validator node VAL_1 calls the function "new" on the template "counter" and gets a valid response
