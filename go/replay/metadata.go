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
	"encoding/json"
	"fmt"
	"log/slog"
	"math/big"

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

	// GetUpgrades returns all stored upgrades.
	GetUpgrades() []opera.UpgradeHeight

	// GetUpgradesAtBlock returns the effective upgrades at the given block number,
	// based on all stored upgrades up to and including that block.
	GetUpgradesAtBlock(blockNumber uint64) opera.Upgrades

	// GetCorrections returns the account corrections to be applied at the
	// given block number, or nil if there are none.
	GetCorrections(blockNumber uint64) map[common.Address]Correction
}

// StaticMetadataStore is a MetadataStore implementation backed by hard-coded
// data for known chains.
type StaticMetadataStore struct {
	metadata     Metadata
	nextUpgrades *opera.Upgrades
}

// NewStaticMetadataStore creates a new StaticMetadataStore with upgrades
// for the given chain ID.
func NewStaticMetadataStore(chainID uint64, logger utils.Logger) (*StaticMetadataStore, error) {
	allegro := opera.GetAllegroUpgrades()
	allegro.GasSubsidies = true
	switch chainID {
	case SonicMainNetChainID:
		corrections, err := GetSonicMainnetCorrections()
		if err != nil {
			return nil, err
		}
		return &StaticMetadataStore{
			metadata: Metadata{
				Upgrades: []opera.UpgradeHeight{
					// Rule update transaction is part of block 56477897,
					// followed by an Epoch seal in block 56477967.
					// The upgrade has affect from and including the following block.
					{Upgrades: allegro, Height: 56477968},
				},
				Corrections: corrections,
			},
		}, nil
	case AllegroTestNetChainID:
		// The Allegro Testnet does not need any corrections, but it does have
		// several network rule upgrades.
		allegroSingleProposer := allegro
		allegroSingleProposer.SingleProposerBlockFormation = true
		return &StaticMetadataStore{
			metadata: Metadata{
				Upgrades: []opera.UpgradeHeight{
					{Upgrades: allegro, Height: 10517},
					{Upgrades: allegroSingleProposer, Height: 16848},
					{Upgrades: allegro, Height: 45517},
					{Upgrades: allegroSingleProposer, Height: 49189},
					{Upgrades: allegro, Height: 51558},
					{Upgrades: allegroSingleProposer, Height: 61156},
					{Upgrades: allegro, Height: 61595},
					{Upgrades: allegroSingleProposer, Height: 63080},
					{Upgrades: allegro, Height: 63374},
					{Upgrades: allegroSingleProposer, Height: 90410},
					{Upgrades: allegro, Height: 106861},
					{Upgrades: allegroSingleProposer, Height: 161033},
					{Upgrades: allegro, Height: 161900},
					{Upgrades: allegroSingleProposer, Height: 251426},
					{Upgrades: allegro, Height: 253299},
				},
			},
		}, nil
	default:
		logger.Warn("No metadata available for chain ID, proceeding without upgrades or corrections", "chainId", chainID)
		return &StaticMetadataStore{}, nil
	}
}

func (s *StaticMetadataStore) PatchUpgrades(blockNumber uint64, diff []byte) error {
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

func (s *StaticMetadataStore) CommitUpgrades(blockNumber uint64) error {
	if s.nextUpgrades == nil {
		return nil
	}
	upgrade := opera.UpgradeHeight{
		Upgrades: *s.nextUpgrades,
		Height:   idx.Block(blockNumber + 1), // effective from next block on
	}
	s.nextUpgrades = nil
	// Verify that the upgrade matches one of the hard-coded values
	for _, existingUpgrade := range s.metadata.Upgrades {
		if existingUpgrade.Height == upgrade.Height &&
			existingUpgrade.Upgrades == upgrade.Upgrades {
			slog.Info("Committed upgrade", "height", upgrade.Height, "upgrades", upgrade.Upgrades)
			return nil
		}
	}
	return fmt.Errorf("upgrade at height %d does not match any hard-coded values", blockNumber+1)
}

// GetUpgrades returns all hard-coded upgrades.
func (s *StaticMetadataStore) GetUpgrades() []opera.UpgradeHeight {
	return s.metadata.Upgrades
}

// GetUpgradesAtBlock returns the effective upgrades at the given block number.
func (s *StaticMetadataStore) GetUpgradesAtBlock(blockNumber uint64) opera.Upgrades {
	upgrades := opera.Upgrades{}
	for _, upgrade := range s.metadata.Upgrades {
		if upgrade.Height <= idx.Block(blockNumber) {
			upgrades = upgrade.Upgrades
		}
	}
	return upgrades
}

// GetCorrections returns the account corrections for the given block number.
func (s *StaticMetadataStore) GetCorrections(blockNumber uint64) map[common.Address]Correction {
	return s.metadata.Corrections[blockNumber]
}

const (
	SonicMainNetChainID   = 146
	AllegroTestNetChainID = 14601
)

// Metadata holds chain-specific metadata such as upgrades to the EVM rules and
// corrections to be applied to account states.
type Metadata struct {
	Upgrades    []opera.UpgradeHeight
	Corrections Corrections
}
