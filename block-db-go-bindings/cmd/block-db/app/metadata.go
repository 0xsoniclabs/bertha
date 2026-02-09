package app

import (
	"log/slog"

	"github.com/0xsoniclabs/sonic/opera"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
)

const (
	SonicMainNetChainId   = 146
	AllegroTestNetChainId = 14601
)

// Metadata holds chain-specific metadata such as upgrades to the EVM rules and
// corrections to be applied to account states.
type Metadata struct {
	Upgrades    []opera.UpgradeHeight
	Corrections Corrections
}

// GetRulesAtBlock returns the EVM rules that should be applied at the given
// block number, based on the upgrades specified in the metadata.
func (m Metadata) GetRulesAtBlock(blockNumber uint64) opera.Rules {
	rules := opera.Rules{}
	for _, upgrade := range m.Upgrades {
		if upgrade.Height <= idx.Block(blockNumber) {
			rules.Upgrades = upgrade.Upgrades
		}
	}
	return rules
}

// GetMetadataForChain retrieves the metadata for a given chain ID.
// TODO: retrieve this data from a Bertha server instead of hardcoding it.
func GetMetadataForChain(chainId uint64) (Metadata, error) {
	allegro := opera.GetAllegroUpgrades()
	switch chainId {
	case SonicMainNetChainId:
		corrections, err := GetSonicMainnetCorrections()
		if err != nil {
			return Metadata{}, err
		}
		return Metadata{
			Upgrades: []opera.UpgradeHeight{
				// Rule update transaction is part of block 56477897,
				// followed by an Epoch seal in block 56477967.
				// The upgrade has affect from and including the following block.
				{Upgrades: allegro, Height: 56477968},
			},
			Corrections: corrections,
		}, nil
	case AllegroTestNetChainId:
		// The Allegro Testnet does not need any corrections, but it does have
		// several network rule upgrades.
		allegroSingleProposer := allegro
		allegroSingleProposer.SingleProposerBlockFormation = true
		return Metadata{
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
		}, nil
	default:
		slog.Warn("no metadata available for chain ID, proceeding without upgrades or corrections", "chainId", chainId)
		return Metadata{}, nil
	}
}
