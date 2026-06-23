// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

package replay

import (
	"cmp"
	"encoding/json"
	"fmt"
	"math/big"
	"reflect"
	"slices"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
)

//go:generate mockgen -source=metadata.go -destination=metadata_mock.go -package=replay

// MetadataStore is an interface for storing and retrieving chain rules and
// corrections.
type MetadataStore interface {
	// PatchRules patches the rules that will be committed by CommitRules with
	// the provided json opera.Rules patch.
	PatchRules(blockNumber uint64, diff []byte) error

	// CommitRules commits the rules that have been updated by PatchRules and
	// will become effective at the next block.
	CommitRules(blockNumber uint64) error

	// GetUpgradeHeights returns all stored upgrade heights.
	GetUpgradeHeights() []opera.UpgradeHeight

	// GetUpgradesAtBlock returns the effective upgrades at the given block number,
	// based on all stored upgrades up to and including that block.
	GetUpgradesAtBlock(blockNumber uint64) opera.Upgrades

	// GetCorrectionsAtBlock returns the account corrections to be applied at the
	// given block number, or nil if there are none.
	GetCorrectionsAtBlock(blockNumber uint64) map[common.Address]Correction
}

// BlockDBMetadataStore is a MetadataStore implementation backed by the block database.
type BlockDBMetadataStore struct {
	db                      blockdb.BlockDB
	metadata                Metadata
	nextRules               *opera.Rules
	logger                  utils.Logger
	writeRulesUpdateHeights bool
}

// NewBlockDBMetadataStore creates a BlockDBMetadataStore backed by data stored in the
// given block database. If writeRulesHeights is true, newly detected rule heights
// are persisted to the block database.
func NewBlockDBMetadataStore(db blockdb.BlockDB, rules opera.Rules, logger utils.Logger, writeRulesHeights bool) (*BlockDBMetadataStore, error) {
	rulesUpdateHeightsData, err := db.GetRulesUpdateHeights(rules.NetworkID)
	if err != nil {
		return nil, fmt.Errorf("failed to get rules update heights from block db: %w", err)
	}
	correctionsData, err := db.GetCorrections(rules.NetworkID)
	if err != nil {
		return nil, fmt.Errorf("failed to get corrections from block db: %w", err)
	}

	var rulesUpdateHeights []RulesUpdateHeight
	if rulesUpdateHeightsData != nil {
		if err := json.Unmarshal(rulesUpdateHeightsData, &rulesUpdateHeights); err != nil {
			return nil, fmt.Errorf("failed to parse stored rules update heights: %w", err)
		}
		logger.Info("Loaded rules update heights from block db", "num_rules_update_heights", len(rulesUpdateHeights))
	} else {
		logger.Warn("No rules update heights available")
	}

	slices.SortFunc(rulesUpdateHeights, func(a, b RulesUpdateHeight) int {
		return cmp.Compare(a.Block, b.Block)
	})

	// Ensure the genesis rules entry is present at block 0.
	if len(rulesUpdateHeights) > 0 && rulesUpdateHeights[0].Block == 0 {
		if !reflect.DeepEqual(rulesUpdateHeights[0].Rules, rules) {
			return nil, fmt.Errorf("stored genesis rules at block 0 do not match provided genesis rules")
		}
	} else {
		rulesUpdateHeights = append([]RulesUpdateHeight{{Block: 0, Rules: rules}}, rulesUpdateHeights...)
	}

	var corrections Corrections
	if correctionsData != nil {
		if err := json.Unmarshal(correctionsData, &corrections); err != nil {
			return nil, fmt.Errorf("failed to parse stored corrections: %w", err)
		}
		logger.Info("Loaded corrections from block db", "num_corrections", len(corrections))
	} else {
		logger.Warn("No corrections available")
	}

	return &BlockDBMetadataStore{
		db: db,
		metadata: Metadata{
			RulesUpdateHeights: rulesUpdateHeights,
			Corrections:        corrections,
		},
		logger:                  logger,
		writeRulesUpdateHeights: writeRulesHeights,
	}, nil
}

func (s *BlockDBMetadataStore) PatchRules(blockNumber uint64, diff []byte) error {
	last := s.metadata.RulesUpdateHeights[0].Rules
	for _, rulesHeight := range s.metadata.RulesUpdateHeights {
		if rulesHeight.Block <= blockNumber {
			last = rulesHeight.Rules
		}
	}
	if s.nextRules != nil {
		last = *s.nextRules
	}
	if last.Economy.MinGasPrice == nil {
		last.Economy.MinGasPrice = new(big.Int)
	}
	if last.Economy.MinBaseFee == nil {
		last.Economy.MinBaseFee = new(big.Int)
	}
	updatedRules, err := opera.UpdateRules(last, diff)
	if err != nil {
		return fmt.Errorf("failed to update rules: %w", err)
	}
	if !reflect.DeepEqual(updatedRules, last) {
		s.nextRules = &updatedRules
	}
	return nil
}

func (s *BlockDBMetadataStore) CommitRules(blockNumber uint64) error {
	if s.nextRules == nil {
		return nil
	}
	rulesUpdateHeight := RulesUpdateHeight{
		Block: blockNumber + 1, // effective from next block on
		Rules: *s.nextRules,
	}
	s.nextRules = nil
	// Check if the rules update matches one of the known values
	for _, existingUpdate := range s.metadata.RulesUpdateHeights {
		if existingUpdate.Block == rulesUpdateHeight.Block {
			if reflect.DeepEqual(existingUpdate.Rules, rulesUpdateHeight.Rules) {
				s.logger.Info("Detected known rules update", "block", rulesUpdateHeight.Block)
				return nil
			} else {
				return fmt.Errorf("unexpected rules update at block %d: update does not match known update", rulesUpdateHeight.Block)
			}
		}
	}
	s.metadata.RulesUpdateHeights = append(s.metadata.RulesUpdateHeights, rulesUpdateHeight)
	slices.SortFunc(s.metadata.RulesUpdateHeights, func(a, b RulesUpdateHeight) int {
		return cmp.Compare(a.Block, b.Block)
	})
	if !s.writeRulesUpdateHeights {
		s.logger.Warn("New rules update detected but not stored in the block db (use --write-rules-update-heights to persist)", "block", rulesUpdateHeight.Block)
		return nil
	}
	data, err := json.Marshal(s.metadata.RulesUpdateHeights)
	if err != nil {
		return fmt.Errorf("failed to marshal rules update heights: %w", err)
	}
	if err := s.db.PutRulesUpdateHeights(s.metadata.RulesUpdateHeights[0].Rules.NetworkID, data); err != nil {
		return fmt.Errorf("failed to store rules update heights in block db: %w", err)
	}
	s.logger.Info("New rules update detected and stored in the block db", "block", rulesUpdateHeight.Block)
	return nil
}

// GetUpgradeHeights returns all stored upgrade heights.
func (s *BlockDBMetadataStore) GetUpgradeHeights() []opera.UpgradeHeight {
	upgradeHeights := make([]opera.UpgradeHeight, len(s.metadata.RulesUpdateHeights))
	for i, rulesHeight := range s.metadata.RulesUpdateHeights {
		upgradeHeights[i] = opera.UpgradeHeight{
			Height:   idx.Block(rulesHeight.Block),
			Upgrades: rulesHeight.Rules.Upgrades,
		}
	}
	return upgradeHeights
}

// GetUpgradesAtBlock returns the effective upgrades at the given block number.
func (s *BlockDBMetadataStore) GetUpgradesAtBlock(blockNumber uint64) opera.Upgrades {
	upgrades := opera.Upgrades{}
	for _, rulesHeight := range s.metadata.RulesUpdateHeights {
		if idx.Block(rulesHeight.Block) <= idx.Block(blockNumber) {
			upgrades = rulesHeight.Rules.Upgrades
		}
	}
	return upgrades
}

// GetCorrectionsAtBlock returns the account corrections for the given block number.
func (s *BlockDBMetadataStore) GetCorrectionsAtBlock(blockNumber uint64) map[common.Address]Correction {
	return s.metadata.Corrections[blockNumber]
}

// Metadata holds chain-specific metadata consisting of the heights of rules
// updates and corrections for the account state.
type Metadata struct {
	RulesUpdateHeights []RulesUpdateHeight
	Corrections        Corrections
}

// RulesUpdateHeight represents a rules update that becomes effective at a specific block.
type RulesUpdateHeight struct {
	Block uint64
	Rules opera.Rules
}
