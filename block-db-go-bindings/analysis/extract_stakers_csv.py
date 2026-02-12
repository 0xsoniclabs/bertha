import json
import csv

def hex_to_dec(hex_str):
    return int(hex_str, 16)

with open('analysis/stakers.json') as f:
    data = json.load(f)

with open('analysis/stakers.csv', 'w', newline='') as csvfile:
    writer = csv.writer(csvfile)
    writer.writerow(['ID', 'SelfStake', 'TotalStake'])
    for staker in data['data']['stakers']:
        id = staker['id']
        self_stake = hex_to_dec(staker['stake'])
        total_stake = hex_to_dec(staker['totalStake'])
        writer.writerow([id, self_stake, total_stake])
