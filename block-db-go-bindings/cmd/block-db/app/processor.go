package app

import (
	"github.com/0xsoniclabs/tosca/go/tosca"
)

type Processor struct {
	processor tosca.Processor
}

func (p Processor) Apply(
	blockParams tosca.BlockParameters,
	transaction tosca.Transaction,
	context tosca.TransactionContext,
) (tosca.Receipt, error) {
	return p.processor.Run(blockParams, transaction, context)
}
