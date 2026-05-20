# pragma version >=0.4.3,<0.6.0

MINIMUM_LIQUIDITY: constant(uint256) = 1000
FEE_TO: constant(address) = 0x0000000000000000000000000000000000000FEE

factory: public(address)
feeTo: public(address)
initialized: public(bool)
reserve0: uint256
reserve1: uint256
blockTimestampLast: uint256
currentTimestamp: uint256
price0CumulativeLast: public(uint256)
price1CumulativeLast: public(uint256)
kLast: public(uint256)
balance0: public(uint256)
balance1: public(uint256)
totalSupply: public(uint256)
balanceOf: public(HashMap[address, uint256])
allowance: public(HashMap[address, HashMap[address, uint256]])

event Approval:
    owner: indexed(address)
    spender: indexed(address)
    amount: uint256

event Transfer:
    sender: indexed(address)
    receiver: indexed(address)
    amount: uint256

event Mint:
    sender: indexed(address)
    amount0: uint256
    amount1: uint256
    liquidity: uint256

event Burn:
    sender: indexed(address)
    amount0: uint256
    amount1: uint256
    receiver: indexed(address)

event Swap:
    sender: indexed(address)
    amount0In: uint256
    amount1In: uint256
    amount0Out: uint256
    amount1Out: uint256
    receiver: indexed(address)

event Sync:
    reserve0: uint256
    reserve1: uint256

@external
def initialize(fee_on: bool) -> bool:
    assert not self.initialized, "initialized"
    self.initialized = True
    self.factory = msg.sender
    self.currentTimestamp = 1
    self.blockTimestampLast = 1
    if fee_on:
        self.feeTo = FEE_TO
    return True

@external
def seedBalances(amount0: uint256, amount1: uint256) -> bool:
    self._ready()
    self.balance0 += amount0
    self.balance1 += amount1
    return True

@external
def setFeeTo(new_fee_to: address) -> bool:
    self._ready()
    assert msg.sender == self.factory, "forbidden"
    self.feeTo = new_fee_to
    if new_fee_to == empty(address):
        self.kLast = 0
    return True

@external
def advanceTime(seconds_forward: uint256) -> uint256:
    self._ready()
    self.currentTimestamp += seconds_forward
    return self.currentTimestamp

@external
def approve(spender: address, amount: uint256) -> bool:
    self.allowance[msg.sender][spender] = amount
    log Approval(owner=msg.sender, spender=spender, amount=amount)
    return True

@external
def transfer(receiver: address, amount: uint256) -> bool:
    self._transfer(msg.sender, receiver, amount)
    return True

@external
def transferFrom(owner: address, receiver: address, amount: uint256) -> bool:
    allowed: uint256 = self.allowance[owner][msg.sender]
    if allowed != max_value(uint256):
        assert allowed >= amount, "allowance"
        self.allowance[owner][msg.sender] = allowed - amount
    self._transfer(owner, receiver, amount)
    return True

@external
@view
def getReserves() -> (uint112, uint112, uint32):
    return convert(self.reserve0, uint112), convert(self.reserve1, uint112), convert(self.blockTimestampLast, uint32)

@external
def mint(receiver: address) -> uint256:
    self._ready()
    old_reserve0: uint256 = self.reserve0
    old_reserve1: uint256 = self.reserve1
    amount0: uint256 = self.balance0 - old_reserve0
    amount1: uint256 = self.balance1 - old_reserve1
    fee_on: bool = self._mint_fee(old_reserve0, old_reserve1)
    supply: uint256 = self.totalSupply
    liquidity: uint256 = 0
    if supply == 0:
        liquidity = self._sqrt(amount0 * amount1) - MINIMUM_LIQUIDITY
        self._mint(empty(address), MINIMUM_LIQUIDITY)
    else:
        liquidity = self._min(amount0 * supply // old_reserve0, amount1 * supply // old_reserve1)
    assert liquidity > 0, "liquidity"
    self._mint(receiver, liquidity)
    self._update(self.balance0, self.balance1, old_reserve0, old_reserve1)
    if fee_on:
        self.kLast = self.reserve0 * self.reserve1
    log Mint(sender=msg.sender, amount0=amount0, amount1=amount1, liquidity=liquidity)
    return liquidity

@external
def stageBurn(liquidity: uint256) -> bool:
    self._ready()
    self._transfer(msg.sender, self, liquidity)
    return True

@external
def burn(receiver: address) -> (uint256, uint256):
    self._ready()
    old_reserve0: uint256 = self.reserve0
    old_reserve1: uint256 = self.reserve1
    fee_on: bool = self._mint_fee(old_reserve0, old_reserve1)
    liquidity: uint256 = self.balanceOf[self]
    supply: uint256 = self.totalSupply
    amount0: uint256 = liquidity * self.balance0 // supply
    amount1: uint256 = liquidity * self.balance1 // supply
    assert amount0 > 0 and amount1 > 0, "liquidity"
    self._burn(self, liquidity)
    self.balance0 -= amount0
    self.balance1 -= amount1
    self._update(self.balance0, self.balance1, old_reserve0, old_reserve1)
    if fee_on:
        self.kLast = self.reserve0 * self.reserve1
    log Burn(sender=msg.sender, amount0=amount0, amount1=amount1, receiver=receiver)
    return amount0, amount1

@external
def swap(amount0Out: uint256, amount1Out: uint256, amount0In: uint256, amount1In: uint256, receiver: address) -> bool:
    self._ready()
    assert amount0Out > 0 or amount1Out > 0, "output"
    old_reserve0: uint256 = self.reserve0
    old_reserve1: uint256 = self.reserve1
    assert amount0Out < old_reserve0 and amount1Out < old_reserve1, "liquidity"
    assert amount0In > 0 or amount1In > 0, "input"
    self.balance0 = self.balance0 + amount0In - amount0Out
    self.balance1 = self.balance1 + amount1In - amount1Out
    balance0_adjusted: uint256 = self.balance0 * 1000 - amount0In * 3
    balance1_adjusted: uint256 = self.balance1 * 1000 - amount1In * 3
    assert balance0_adjusted * balance1_adjusted >= old_reserve0 * old_reserve1 * 1000000, "k"
    self._update(self.balance0, self.balance1, old_reserve0, old_reserve1)
    log Swap(sender=msg.sender, amount0In=amount0In, amount1In=amount1In, amount0Out=amount0Out, amount1Out=amount1Out, receiver=receiver)
    return True

@external
def skim(receiver: address) -> (uint256, uint256):
    self._ready()
    amount0: uint256 = self.balance0 - self.reserve0
    amount1: uint256 = self.balance1 - self.reserve1
    self.balance0 = self.reserve0
    self.balance1 = self.reserve1
    log Burn(sender=msg.sender, amount0=amount0, amount1=amount1, receiver=receiver)
    return amount0, amount1

@external
def sync() -> bool:
    self._ready()
    self._update(self.balance0, self.balance1, self.reserve0, self.reserve1)
    return True

@internal
@view
def _ready():
    assert self.initialized, "not initialized"

@internal
def _transfer(owner: address, receiver: address, amount: uint256):
    assert self.balanceOf[owner] >= amount, "balance"
    self.balanceOf[owner] -= amount
    self.balanceOf[receiver] += amount
    log Transfer(sender=owner, receiver=receiver, amount=amount)

@internal
def _mint(receiver: address, amount: uint256):
    self.totalSupply += amount
    self.balanceOf[receiver] += amount
    log Transfer(sender=empty(address), receiver=receiver, amount=amount)

@internal
def _burn(owner: address, amount: uint256):
    assert self.balanceOf[owner] >= amount, "balance"
    self.balanceOf[owner] -= amount
    self.totalSupply -= amount
    log Transfer(sender=owner, receiver=empty(address), amount=amount)

@internal
def _update(new_balance0: uint256, new_balance1: uint256, old_reserve0: uint256, old_reserve1: uint256):
    assert new_balance0 <= convert(max_value(uint112), uint256) and new_balance1 <= convert(max_value(uint112), uint256), "overflow"
    time_elapsed: uint256 = self.currentTimestamp - self.blockTimestampLast
    if time_elapsed > 0 and old_reserve0 != 0 and old_reserve1 != 0:
        self.price0CumulativeLast += old_reserve1 * 10**18 // old_reserve0 * time_elapsed
        self.price1CumulativeLast += old_reserve0 * 10**18 // old_reserve1 * time_elapsed
    self.reserve0 = new_balance0
    self.reserve1 = new_balance1
    self.blockTimestampLast = self.currentTimestamp
    log Sync(reserve0=new_balance0, reserve1=new_balance1)

@internal
def _mint_fee(old_reserve0: uint256, old_reserve1: uint256) -> bool:
    current_fee_to: address = self.feeTo
    fee_on: bool = current_fee_to != empty(address)
    last_k: uint256 = self.kLast
    if fee_on:
        if last_k != 0:
            root_k: uint256 = self._sqrt(old_reserve0 * old_reserve1)
            root_k_last: uint256 = self._sqrt(last_k)
            if root_k > root_k_last:
                liquidity: uint256 = self.totalSupply * (root_k - root_k_last) // (root_k * 5 + root_k_last)
                if liquidity > 0:
                    self._mint(current_fee_to, liquidity)
    elif last_k != 0:
        self.kLast = 0
    return fee_on

@internal
@pure
def _sqrt(y: uint256) -> uint256:
    z: uint256 = 0
    if y > 3:
        z = y
        x: uint256 = y // 2 + 1
        for _: uint256 in range(256):
            if x >= z:
                break
            z = x
            x = (y // x + x) // 2
    elif y != 0:
        z = 1
    return z

@internal
@pure
def _min(x: uint256, y: uint256) -> uint256:
    if x < y:
        return x
    return y
