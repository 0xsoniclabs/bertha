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
	"fmt"
	"slices"

	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
)

//go:generate mockgen -source=metadata.go -destination=metadata_mock.go -package=replay

// MetadataStore is an interface for storing and retrieving chain upgrade rules
// and corrections.
type MetadataStore interface {
	// StoreUpgrade stores an upgrade that takes effect at the given block height.
	StoreUpgrade(upgrade opera.UpgradeHeight) error

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
	metadata Metadata
}

// NewStaticMetadataStore creates a new StaticMetadataStore with upgrades
// for the given chain ID.
func NewStaticMetadataStore(chainID uint64, logger utils.Logger) (*StaticMetadataStore, error) {
	allegro := opera.GetAllegroUpgrades()
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
		logger.Warn("no metadata available for chain ID, proceeding without upgrades or corrections", "chainId", chainID)
		return &StaticMetadataStore{}, nil
	}
}

// StoreUpgrade verifies that the provided upgrade matches one of the
// hard-coded upgrade values. Returns an error if it does not match.
func (s *StaticMetadataStore) StoreUpgrade(upgrade opera.UpgradeHeight) error {
	if !slices.Contains(s.metadata.Upgrades, upgrade) {
		return fmt.Errorf("upgrade at height %d does not match any hard-coded values", upgrade.Height)
	}
	return nil
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
