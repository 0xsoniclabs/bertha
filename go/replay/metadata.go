// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

package replay

import (
	"cmp"
	"encoding/json"
	"fmt"
	"math/big"
	"slices"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
)

//go:generate mockgen -source=metadata.go -destination=metadata_mock.go -package=replay

// MetadataStore is an interface for storing and retrieving chain upgrade rules
// and corrections.
type MetadataStore interface {
	// PatchUpgrades patches the upgrades that will be committed by
	// CommitUpgrades with the provided json opera.Rules patch.
	PatchUpgrades(blockNumber uint64, diff []byte) error

	// CommitUpgrades commits the upgrades that have been updated by
	// PatchUpgrades and will become effective at the next block.
	CommitUpgrades(blockNumber uint64) error

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
	db                  blockdb.BlockDB
	metadata            Metadata
	nextUpgrades        *opera.Upgrades
	chainID             uint64
	logger              utils.Logger
	writeUpgradeHeights bool
}

// NewBlockDBMetadataStore creates a BlockDBMetadataStore backed by data stored in the
// given block database. If writeUpgradeHeights is true, newly detected upgrade heights
// are persisted to the block database.
func NewBlockDBMetadataStore(db blockdb.BlockDB, chainID uint64, logger utils.Logger, writeUpgradeHeights bool) (*BlockDBMetadataStore, error) {
	upgradesData, err := db.GetUpgradeHeights(chainID)
	if err != nil {
		return nil, fmt.Errorf("failed to get upgrade heights from block db: %w", err)
	}
	correctionsData, err := db.GetCorrections(chainID)
	if err != nil {
		return nil, fmt.Errorf("failed to get corrections from block db: %w", err)
	}

	var upgrades []opera.UpgradeHeight
	if upgradesData != nil {
		if err := json.Unmarshal(upgradesData, &upgrades); err != nil {
			return nil, fmt.Errorf("failed to parse stored upgrade heights: %w", err)
		}
		logger.Info("Loaded upgrade heights from block db", "num_upgrade_heights", len(upgrades))
	} else {
		logger.Warn("No upgrade heights available")
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
			UpgradeHeights: upgrades,
			Corrections:    corrections,
		},
		chainID:             chainID,
		logger:              logger,
		writeUpgradeHeights: writeUpgradeHeights,
	}, nil
}

func (s *BlockDBMetadataStore) PatchUpgrades(blockNumber uint64, diff []byte) error {
	var upgrades opera.Upgrades
	if s.nextUpgrades != nil {
		upgrades = *s.nextUpgrades
	} else {
		upgrades = s.GetUpgradesAtBlock(blockNumber)
		// Only Sonic modifies its updates by log messages, and Sonic has the
		// updates Berlin, London and Sonic enabled from the beginning, and
		// therefore there are no upgrade heights for them.
		upgrades.Berlin = true
		upgrades.London = true
		upgrades.Sonic = true
	}
	originalRules := opera.Rules{
		Economy:  opera.EconomyRules{MinGasPrice: &big.Int{}, MinBaseFee: &big.Int{}},
		Upgrades: upgrades,
	}
	updatedRules, err := updateRules(originalRules, diff)
	if err != nil {
		return fmt.Errorf("failed to update rules: %v", err)
	}
	if updatedRules.Upgrades != originalRules.Upgrades {
		s.nextUpgrades = &updatedRules.Upgrades
	}
	return nil
}

func updateRules(src opera.Rules, diff []byte) (opera.Rules, error) {
	changed := src.Copy()
	if err := json.Unmarshal(diff, &changed); err != nil {
		return opera.Rules{}, err
	}

	// protect readonly fields
	changed.NetworkID = src.NetworkID
	changed.Name = src.Name

	return changed, nil
}

func (s *BlockDBMetadataStore) CommitUpgrades(blockNumber uint64) error {
	if s.nextUpgrades == nil {
		return nil
	}
	upgrade := opera.UpgradeHeight{
		Upgrades: *s.nextUpgrades,
		Height:   idx.Block(blockNumber + 1), // effective from next block on
	}
	s.nextUpgrades = nil
	// Check if the upgrade matches one of the known values
	for _, existingUpgrade := range s.metadata.UpgradeHeights {
		if existingUpgrade.Height == upgrade.Height {
			if existingUpgrade.Upgrades == upgrade.Upgrades {
				s.logger.Info("Detected known upgrade", "block", upgrade.Height)
				return nil
			} else {
				return fmt.Errorf("unexpected upgrade at block %d: upgrade does not match known upgrade", upgrade.Height)
			}
		}
	}
	s.metadata.UpgradeHeights = append(s.metadata.UpgradeHeights, upgrade)
	slices.SortFunc(s.metadata.UpgradeHeights, func(a, b opera.UpgradeHeight) int {
		return cmp.Compare(a.Height, b.Height)
	})
	if !s.writeUpgradeHeights {
		s.logger.Warn("New upgrade detected but not stored in the block db (use --write-upgrade-heights to persist)", "block", upgrade.Height)
		return nil
	}
	data, err := json.Marshal(s.metadata.UpgradeHeights)
	if err != nil {
		return fmt.Errorf("failed to marshal upgrade heights: %w", err)
	}
	if err := s.db.PutUpgradeHeights(s.chainID, data); err != nil {
		return fmt.Errorf("failed to store upgrade heights in block db: %w", err)
	}
	s.logger.Info("New upgrade detected and stored in the block db", "block", upgrade.Height)
	return nil
}

// GetUpgradeHeights returns all stored upgrade heights.
func (s *BlockDBMetadataStore) GetUpgradeHeights() []opera.UpgradeHeight {
	return s.metadata.UpgradeHeights
}

// GetUpgradesAtBlock returns the effective upgrades at the given block number.
func (s *BlockDBMetadataStore) GetUpgradesAtBlock(blockNumber uint64) opera.Upgrades {
	upgrades := opera.Upgrades{}
	for _, upgrade := range s.metadata.UpgradeHeights {
		if upgrade.Height <= idx.Block(blockNumber) {
			upgrades = upgrade.Upgrades
		}
	}
	return upgrades
}

// GetCorrectionsAtBlock returns the account corrections for the given block number.
func (s *BlockDBMetadataStore) GetCorrectionsAtBlock(blockNumber uint64) map[common.Address]Correction {
	return s.metadata.Corrections[blockNumber]
}

// Metadata holds chain-specific metadata such as upgrades to the EVM rules and
// corrections to be applied to account states.
type Metadata struct {
	UpgradeHeights []opera.UpgradeHeight
	Corrections    Corrections
}
