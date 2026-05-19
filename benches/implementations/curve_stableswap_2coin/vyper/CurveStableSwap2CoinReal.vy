# pragma version 0.4.3

N_COINS: constant(uint256) = 2
A_PRECISION: constant(uint256) = 100
FEE_DENOMINATOR: constant(uint256) = 10000000000

A: public(uint256)
fee: public(uint256)
admin_fee: public(uint256)
initialized: public(bool)
balances: public(uint256[2])
admin_balances: public(uint256[2])
totalSupply: public(uint256)
balanceOf: public(HashMap[address, uint256])

event AddLiquidity:
    provider: indexed(address)
    amount0: uint256
    amount1: uint256
    minted: uint256
    invariant: uint256

event TokenExchange:
    buyer: indexed(address)
    sold_id: uint256
    tokens_sold: uint256
    bought_id: uint256
    tokens_bought: uint256

event RemoveLiquidity:
    provider: indexed(address)
    amount0: uint256
    amount1: uint256
    burned: uint256

event RemoveLiquidityOne:
    provider: indexed(address)
    token_amount: uint256
    coin_index: uint256
    coin_amount: uint256

event Transfer:
    sender: indexed(address)
    receiver: indexed(address)
    amount: uint256

@external
def initialize(amp: uint256, swap_fee: uint256, admin_fee_: uint256) -> bool:
    assert not self.initialized, "initialized"
    assert amp > 0, "amp"
    assert swap_fee <= FEE_DENOMINATOR // 10, "fee"
    assert admin_fee_ <= FEE_DENOMINATOR, "admin"
    self.initialized = True
    self.A = amp
    self.fee = swap_fee
    self.admin_fee = admin_fee_
    return True

@external
def add_liquidity(amount0: uint256, amount1: uint256, min_mint_amount: uint256) -> uint256:
    self._ready()
    assert amount0 > 0 or amount1 > 0, "amount"
    d0: uint256 = 0
    if self.totalSupply != 0:
        d0 = self._getD(self.balances[0], self.balances[1])
    self.balances[0] += amount0
    self.balances[1] += amount1
    d1: uint256 = self._getD(self.balances[0], self.balances[1])
    minted: uint256 = d1
    if self.totalSupply != 0:
        minted = self.totalSupply * (d1 - d0) // d0
    assert minted >= min_mint_amount and minted > 0, "slippage"
    self._mint(msg.sender, minted)
    log AddLiquidity(provider=msg.sender, amount0=amount0, amount1=amount1, minted=minted, invariant=d1)
    return minted

@external
def exchange(i: uint256, j: uint256, dx: uint256, min_dy: uint256) -> uint256:
    self._ready()
    assert i < N_COINS and j < N_COINS and i != j, "coin"
    assert dx > 0, "dx"
    x: uint256 = self.balances[i] + dx
    y: uint256 = self._getY(i, j, x)
    old_y: uint256 = self.balances[j]
    dy: uint256 = old_y - y - 1
    fee_amount: uint256 = dy * self.fee // FEE_DENOMINATOR
    admin_cut: uint256 = fee_amount * self.admin_fee // FEE_DENOMINATOR
    user_dy: uint256 = dy - fee_amount
    assert user_dy >= min_dy, "slippage"
    self.balances[i] = x
    self.balances[j] = old_y - dy + fee_amount - admin_cut
    self.admin_balances[j] += admin_cut
    log TokenExchange(buyer=msg.sender, sold_id=i, tokens_sold=dx, bought_id=j, tokens_bought=user_dy)
    return user_dy

@external
def remove_liquidity(lp_amount: uint256, min0: uint256, min1: uint256) -> (uint256, uint256):
    self._ready()
    assert lp_amount > 0 and self.balanceOf[msg.sender] >= lp_amount, "lp"
    supply: uint256 = self.totalSupply
    amount0: uint256 = self.balances[0] * lp_amount // supply
    amount1: uint256 = self.balances[1] * lp_amount // supply
    assert amount0 >= min0 and amount1 >= min1, "slippage"
    self._burn(msg.sender, lp_amount)
    self.balances[0] -= amount0
    self.balances[1] -= amount1
    log RemoveLiquidity(provider=msg.sender, amount0=amount0, amount1=amount1, burned=lp_amount)
    return amount0, amount1

@external
def remove_liquidity_one_coin(lp_amount: uint256, i: uint256, min_amount: uint256) -> uint256:
    self._ready()
    assert i < N_COINS, "coin"
    assert lp_amount > 0 and self.balanceOf[msg.sender] >= lp_amount, "lp"
    amount: uint256 = self.balances[i] * lp_amount // self.totalSupply
    fee_amount: uint256 = amount * self.fee // FEE_DENOMINATOR
    admin_cut: uint256 = fee_amount * self.admin_fee // FEE_DENOMINATOR
    user_amount: uint256 = amount - fee_amount
    assert user_amount >= min_amount, "slippage"
    self._burn(msg.sender, lp_amount)
    self.balances[i] = self.balances[i] - amount + fee_amount - admin_cut
    self.admin_balances[i] += admin_cut
    log RemoveLiquidityOne(provider=msg.sender, token_amount=lp_amount, coin_index=i, coin_amount=user_amount)
    return user_amount

@external
@view
def get_D(x0: uint256, x1: uint256) -> uint256:
    return self._getD(x0, x1)

@external
@view
def get_virtual_price() -> uint256:
    if self.totalSupply == 0:
        return 10**18
    return self._getD(self.balances[0], self.balances[1]) * 10**18 // self.totalSupply

@internal
@view
def _ready():
    assert self.initialized, "not initialized"

@internal
def _mint(receiver: address, amount: uint256):
    self.totalSupply += amount
    self.balanceOf[receiver] += amount
    log Transfer(sender=empty(address), receiver=receiver, amount=amount)

@internal
def _burn(owner: address, amount: uint256):
    self.balanceOf[owner] -= amount
    self.totalSupply -= amount
    log Transfer(sender=owner, receiver=empty(address), amount=amount)

@internal
@view
def _getD(x0: uint256, x1: uint256) -> uint256:
    s: uint256 = x0 + x1
    if s == 0:
        return 0
    d: uint256 = s
    ann: uint256 = self.A * N_COINS
    for _: uint256 in range(255):
        d_p: uint256 = d * d // (x0 * N_COINS)
        d_p = d_p * d // (x1 * N_COINS)
        previous_d: uint256 = d
        d = (ann * s // A_PRECISION + d_p * N_COINS) * d // ((ann - A_PRECISION) * d // A_PRECISION + (N_COINS + 1) * d_p)
        if d > previous_d:
            if d - previous_d <= 1:
                return d
        elif previous_d - d <= 1:
            return d
    return d

@internal
@view
def _getY(i: uint256, j: uint256, x: uint256) -> uint256:
    d: uint256 = self._getD(self.balances[0], self.balances[1])
    ann: uint256 = self.A * N_COINS
    c: uint256 = d
    s: uint256 = 0
    for idx: uint256 in range(2):
        if idx != j:
            current_x: uint256 = self.balances[idx]
            if idx == i:
                current_x = x
            s += current_x
            c = c * d // (current_x * N_COINS)
    c = c * d * A_PRECISION // (ann * N_COINS)
    b: uint256 = s + d * A_PRECISION // ann
    y: uint256 = d
    for _: uint256 in range(255):
        previous_y: uint256 = y
        y = (y * y + c) // (2 * y + b - d)
        if y > previous_y:
            if y - previous_y <= 1:
                return y
        elif previous_y - y <= 1:
            return y
    return y
