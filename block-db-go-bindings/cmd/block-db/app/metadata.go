package app

import (
	"log/slog"

	"github.com/0xsoniclabs/sonic/opera"
)

const (
	SonicMainNetChainId   = 146
	AllegroTestNetChainId = 14601
)

// Metadata holds chain-specific metadata such upgrades to the EVM rules and
// corrections to be applied to account states.
type Metadata struct {
	Upgrades    []opera.UpgradeHeight
	Corrections Corrections
}

// GetMetadataForChain retrieves the metadata for a given chain ID.
// TODO: retrieve this data from a Bertha server instead of hardcoding it.
func GetMetadataForChain(chainId uint64) (Metadata, error) {
	switch chainId {
	case SonicMainNetChainId:
		corrections, err := GetSonicMainnetCorrections()
		if err != nil {
			return Metadata{}, err
		}
		return Metadata{
			Corrections: corrections,
			// No upgrades for Sonic Mainnet as of now.
		}, nil
	case AllegroTestNetChainId:
		// The Allegro Testnet does not need any corrections, but it does have
		// several network rule upgrades.
		allegro := opera.GetAllegroUpgrades()
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
