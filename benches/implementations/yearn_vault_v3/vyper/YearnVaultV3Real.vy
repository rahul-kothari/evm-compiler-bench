# pragma version 0.4.3

MAX_BPS: constant(uint256) = 10000
WAD: constant(uint256) = 10**18

struct Strategy:
    activation: uint256
    currentDebt: uint256
    maxDebt: uint256
    balance: uint256

struct PendingReport:
    gain: uint256
    loss: uint256

roleManager: public(address)
initialized: public(bool)
shutdown: public(bool)
depositLimit: public(uint256)
profitMaxUnlockTime: public(uint256)
fullProfitUnlockDate: public(uint256)
profitUnlockingRate: public(uint256)
lastProfitUpdate: public(uint256)
currentTimestamp: public(uint256)
feeBps: public(uint256)
totalIdle: public(uint256)
totalDebt: public(uint256)
totalSupply: public(uint256)
defaultQueueStrategy: public(address)
balanceOf: public(HashMap[address, uint256])
allowance: public(HashMap[address, HashMap[address, uint256]])
nonces: public(HashMap[address, uint256])
strategies: HashMap[address, Strategy]
pendingReports: HashMap[address, PendingReport]

event Transfer:
    sender: indexed(address)
    receiver: indexed(address)
    amount: uint256

event Approval:
    owner: indexed(address)
    spender: indexed(address)
    amount: uint256

event Deposit:
    sender: indexed(address)
    owner: indexed(address)
    assets: uint256
    shares: uint256

event Withdraw:
    sender: indexed(address)
    receiver: indexed(address)
    owner: indexed(address)
    assets: uint256
    shares: uint256

event StrategyChanged:
    strategy: indexed(address)
    activation: uint256

event DebtUpdated:
    strategy: indexed(address)
    current_debt: uint256
    new_debt: uint256

event StrategyReported:
    strategy: indexed(address)
    gain: uint256
    loss: uint256
    total_debt: uint256
    total_idle: uint256

event Shutdown:
    pass

@external
def initialize(limit: uint256, unlock_time: uint256, fee_bps: uint256) -> bool:
    assert not self.initialized, "initialized"
    assert fee_bps <= MAX_BPS, "fee"
    self.initialized = True
    self.roleManager = msg.sender
    self.depositLimit = limit
    self.profitMaxUnlockTime = unlock_time
    self.feeBps = fee_bps
    self.currentTimestamp = 1
    self.lastProfitUpdate = 1
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
def deposit(assets: uint256, receiver: address) -> uint256:
    self._ready()
    assert not self.shutdown, "shutdown"
    assert assets > 0, "assets"
    assert self._total_assets() + assets <= self.depositLimit, "limit"
    shares: uint256 = self._convert_to_shares(assets, False)
    assert shares > 0, "shares"
    self.totalIdle += assets
    self._mint(receiver, shares)
    log Deposit(sender=msg.sender, owner=receiver, assets=assets, shares=shares)
    return shares

@external
def mint(shares: uint256, receiver: address) -> uint256:
    self._ready()
    assert not self.shutdown, "shutdown"
    assert shares > 0, "shares"
    assets: uint256 = self._convert_to_assets(shares, True)
    assert self._total_assets() + assets <= self.depositLimit, "limit"
    self.totalIdle += assets
    self._mint(receiver, shares)
    log Deposit(sender=msg.sender, owner=receiver, assets=assets, shares=shares)
    return assets

@external
def withdraw(assets: uint256, receiver: address, owner: address, max_loss: uint256) -> uint256:
    self._ready()
    shares: uint256 = self._convert_to_shares(assets, True)
    actual_assets: uint256 = self._redeem(receiver, owner, assets, shares, max_loss)
    log Withdraw(sender=msg.sender, receiver=receiver, owner=owner, assets=actual_assets, shares=shares)
    return shares

@external
def redeem(shares: uint256, receiver: address, owner: address, max_loss: uint256) -> uint256:
    self._ready()
    assets: uint256 = self._convert_to_assets(shares, False)
    actual_assets: uint256 = self._redeem(receiver, owner, assets, shares, max_loss)
    log Withdraw(sender=msg.sender, receiver=receiver, owner=owner, assets=actual_assets, shares=shares)
    return actual_assets

@external
def add_strategy(strategy: address, add_to_queue: bool) -> bool:
    self._ready()
    self._only_manager()
    assert strategy != empty(address), "strategy"
    assert self.strategies[strategy].activation == 0, "active"
    self.strategies[strategy].activation = self.currentTimestamp
    if add_to_queue:
        self.defaultQueueStrategy = strategy
    log StrategyChanged(strategy=strategy, activation=self.currentTimestamp)
    return True

@external
def update_max_debt_for_strategy(strategy: address, max_debt: uint256) -> bool:
    self._ready()
    self._only_manager()
    assert self.strategies[strategy].activation != 0, "inactive"
    self.strategies[strategy].maxDebt = max_debt
    return True

@external
def update_debt(strategy: address, target_debt: uint256, max_loss: uint256) -> uint256:
    self._ready()
    self._only_manager()
    assert self.strategies[strategy].activation != 0, "inactive"
    assert target_debt <= self.strategies[strategy].maxDebt, "max"
    previous_debt: uint256 = self.strategies[strategy].currentDebt
    if target_debt > previous_debt:
        amount: uint256 = target_debt - previous_debt
        assert self.totalIdle >= amount, "idle"
        self.totalIdle -= amount
        self.totalDebt += amount
        self.strategies[strategy].currentDebt = target_debt
        self.strategies[strategy].balance += amount
    else:
        amount: uint256 = previous_debt - target_debt
        withdrawn: uint256 = self._min(amount, self.strategies[strategy].balance)
        loss: uint256 = amount - withdrawn
        assert amount == 0 or loss * MAX_BPS <= amount * max_loss, "loss"
        self.strategies[strategy].balance -= withdrawn
        self.strategies[strategy].currentDebt = target_debt
        self.totalDebt = self.totalDebt + loss - amount
        self.totalIdle += withdrawn
    log DebtUpdated(strategy=strategy, current_debt=previous_debt, new_debt=self.strategies[strategy].currentDebt)
    return self.strategies[strategy].currentDebt

@external
def mock_strategy_report(strategy: address, gain: uint256, loss: uint256) -> bool:
    self._ready()
    self._only_manager()
    assert self.strategies[strategy].activation != 0, "inactive"
    self.pendingReports[strategy] = PendingReport(gain=gain, loss=loss)
    return True

@external
def process_report(strategy: address) -> (uint256, uint256):
    self._ready()
    self._only_manager()
    assert self.strategies[strategy].activation != 0, "inactive"
    gain: uint256 = self.pendingReports[strategy].gain
    loss: uint256 = self._min(self.pendingReports[strategy].loss, self.strategies[strategy].currentDebt)
    self.pendingReports[strategy] = PendingReport(gain=0, loss=0)
    if loss > 0:
        self.strategies[strategy].currentDebt -= loss
        if self.strategies[strategy].balance > loss:
            self.strategies[strategy].balance -= loss
        else:
            self.strategies[strategy].balance = 0
        self.totalDebt -= loss
    if gain > 0:
        fee: uint256 = gain * self.feeBps // MAX_BPS
        net_gain: uint256 = gain - fee
        shares_to_lock: uint256 = self._convert_to_shares(net_gain, False)
        self.strategies[strategy].currentDebt += gain
        self.strategies[strategy].balance += gain
        self.totalDebt += gain
        if shares_to_lock > 0:
            self._mint(self, shares_to_lock)
            if self.profitMaxUnlockTime != 0:
                self.profitUnlockingRate = shares_to_lock // self.profitMaxUnlockTime
                self.fullProfitUnlockDate = self.currentTimestamp + self.profitMaxUnlockTime
                self.lastProfitUpdate = self.currentTimestamp
        if fee > 0:
            fee_shares: uint256 = self._convert_to_shares(fee, False)
            if fee_shares > 0:
                self._mint(self.roleManager, fee_shares)
    log StrategyReported(strategy=strategy, gain=gain, loss=loss, total_debt=self.totalDebt, total_idle=self.totalIdle)
    return gain, loss

@external
def shutdown_vault() -> bool:
    self._ready()
    self._only_manager()
    self.shutdown = True
    self.depositLimit = 0
    log Shutdown()
    return True

@external
@view
def permitDigest(owner: address, spender: address, amount: uint256, deadline: uint256) -> bytes32:
    return keccak256(abi_encode(amount, self.nonces[owner], deadline))

@external
@view
def totalAssets() -> uint256:
    return self._total_assets()

@external
@view
def pricePerShare() -> uint256:
    supply: uint256 = self._effective_supply()
    if supply == 0:
        return WAD
    return self._total_assets() * WAD // supply

@external
@view
def maxDeposit(receiver: address) -> uint256:
    if self.shutdown or self._total_assets() >= self.depositLimit:
        return 0
    return self.depositLimit - self._total_assets()

@external
@view
def maxWithdraw(owner: address, max_loss: uint256) -> uint256:
    return self._convert_to_assets(self.balanceOf[owner], False)

@external
@view
def strategyState(strategy: address) -> (uint256, uint256, uint256, uint256):
    return (
        self.strategies[strategy].activation,
        self.strategies[strategy].currentDebt,
        self.strategies[strategy].maxDebt,
        self.strategies[strategy].balance,
    )

@internal
@view
def _ready():
    assert self.initialized, "not initialized"

@internal
@view
def _only_manager():
    assert msg.sender == self.roleManager, "permission"

@internal
def _redeem(receiver: address, owner: address, assets: uint256, shares: uint256, max_loss: uint256) -> uint256:
    assert shares > 0 and self.balanceOf[owner] >= shares, "shares"
    actual_assets: uint256 = self._ensure_idle(assets, max_loss)
    self._burn(owner, shares)
    self.totalIdle -= actual_assets
    return actual_assets

@internal
def _ensure_idle(assets: uint256, max_loss: uint256) -> uint256:
    if self.totalIdle >= assets:
        return assets
    strategy: address = self.defaultQueueStrategy
    assert strategy != empty(address), "queue"
    needed: uint256 = assets - self.totalIdle
    withdrawn: uint256 = self._min(needed, self.strategies[strategy].balance)
    self.strategies[strategy].balance -= withdrawn
    self.strategies[strategy].currentDebt -= withdrawn
    self.totalDebt -= withdrawn
    self.totalIdle += withdrawn
    if self.totalIdle < assets:
        loss: uint256 = assets - self.totalIdle
        assert loss * MAX_BPS <= assets * max_loss, "loss"
        debt_loss: uint256 = self._min(loss, self.strategies[strategy].currentDebt)
        total_loss: uint256 = self._min(loss, self.totalDebt)
        self.strategies[strategy].currentDebt -= debt_loss
        self.totalDebt -= total_loss
        return self.totalIdle
    return assets

@internal
@view
def _total_assets() -> uint256:
    return self.totalIdle + self.totalDebt

@internal
@view
def _effective_supply() -> uint256:
    return self.totalSupply - self._unlocked_shares()

@internal
@view
def _unlocked_shares() -> uint256:
    locked: uint256 = self.balanceOf[self]
    if locked == 0:
        return 0
    if self.profitMaxUnlockTime == 0 or self.currentTimestamp >= self.fullProfitUnlockDate:
        return locked
    elapsed: uint256 = self.currentTimestamp - self.lastProfitUpdate
    return self._min(locked, elapsed * self.profitUnlockingRate)

@internal
@view
def _convert_to_shares(assets: uint256, round_up: bool) -> uint256:
    supply: uint256 = self._effective_supply()
    assets_total: uint256 = self._total_assets()
    if supply == 0 or assets_total == 0:
        return assets
    shares: uint256 = assets * supply // assets_total
    if round_up and shares * assets_total < assets * supply:
        shares += 1
    return shares

@internal
@view
def _convert_to_assets(shares: uint256, round_up: bool) -> uint256:
    supply: uint256 = self._effective_supply()
    if supply == 0:
        return shares
    assets: uint256 = shares * self._total_assets() // supply
    if round_up and assets * supply < shares * self._total_assets():
        assets += 1
    return assets

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
def _transfer(owner: address, receiver: address, amount: uint256):
    assert self.balanceOf[owner] >= amount, "balance"
    self.balanceOf[owner] -= amount
    self.balanceOf[receiver] += amount
    log Transfer(sender=owner, receiver=receiver, amount=amount)

@internal
@pure
def _min(a: uint256, b: uint256) -> uint256:
    if a < b:
        return a
    return b
