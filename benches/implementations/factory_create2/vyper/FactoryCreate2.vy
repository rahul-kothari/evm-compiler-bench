# pragma version 0.4.3

deployedValue: public(HashMap[bytes32, uint256])

event Deployed:
    salt: indexed(bytes32)
    predicted: address
    value: uint256

@external
@pure
def initCodeHash(amount: uint256) -> bytes32:
    return keccak256(convert(amount, bytes32))

@internal
@view
def _compute_address(salt: bytes32, amount: uint256) -> address:
    digest: bytes32 = keccak256(concat(0xff, convert(self, bytes20), salt, keccak256(convert(amount, bytes32))))
    return convert(convert(digest, uint256), address)

@external
@view
def computeAddress(salt: bytes32, amount: uint256) -> address:
    return self._compute_address(salt, amount)

@external
def deploy(salt: bytes32, amount: uint256) -> uint256:
    predicted: address = self._compute_address(salt, amount)
    self.deployedValue[salt] = amount
    log Deployed(salt=salt, predicted=predicted, value=amount)
    return amount
